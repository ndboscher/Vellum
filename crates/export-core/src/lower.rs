//! Lowers a [`Document`] into a target-neutral element tree per page plus one set of CSS rules.
//!
//! Every exporter that emits markup (HTML, JSX, Vue templates, ...) prints this
//! tree instead of walking the document itself, so node semantics, class naming,
//! URL sanitizing and style serialization live in one place. The markup and base
//! styles mirror the editor's canvas (`apps/editor/src/page.rs` and
//! `apps/editor/style.css`) so an export looks like what the user built.
//! Per-node declarations come from `vellum_schema` ([`Style::declarations`],
//! [`Layout::declarations`], [`Tokens::variables`]), which the canvas uses too.
//!
//! [`Layout::declarations`]: vellum_schema::Layout::declarations
//! [`Tokens::variables`]: vellum_schema::Tokens::variables

use std::collections::{BTreeMap, HashMap, HashSet};

use vellum_schema::{
    css, Breakpoint, Component, Document, Drawing, Geometry, Node, NodeKind, Page, PropKind, Shape,
    Style,
};
use vellum_vector::path_data;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputNode {
    Element(Element),
    Text(String),
    /// Inside a component template: the value of this prop, as text.
    Prop(String),
    /// A placed component.
    Instance(Instance),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub tag: &'static str,
    /// Attributes in HTML spelling (`class`, `stroke-width`, `viewBox`).
    pub attrs: Vec<(&'static str, String)>,
    /// Inside a component template: attributes whose value is a prop, as attribute to prop name.
    pub prop_attrs: Vec<(&'static str, String)>,
    pub children: Vec<OutputNode>,
}

/// A component placed on a page or inside another component. Targets with components print
/// `component` with `props`; the rest print `expanded`.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// The component's PascalCase name, as in [`LoweredComponent::name`].
    pub component: String,
    /// The props this instance sets, already sanitized, in the component's prop order.
    pub props: Vec<(String, String)>,
    /// The component's markup with this instance's values filled in.
    pub expanded: Element,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredComponent {
    /// A unique PascalCase identifier made from the component's name.
    pub name: String,
    pub props: Vec<LoweredProp>,
    /// The component's markup, with [`OutputNode::Prop`]s and `prop_attrs` where props show.
    pub root: Element,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredProp {
    pub name: String,
    pub kind: PropKind,
    /// Sanitized like an instance's value.
    pub default: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CssRule {
    /// An enclosing at-rule such as `@media (prefers-color-scheme: dark)`.
    pub media: Option<String>,
    pub selector: String,
    pub declarations: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredSite {
    /// In the document's page order; the first is the home page.
    pub pages: Vec<LoweredPage>,
    /// In the document's component order.
    pub components: Vec<LoweredComponent>,
    /// One stylesheet for every page: design tokens as custom properties, base rules shared by
    /// every export, one rule per node, then each breakpoint's overrides, widest first.
    pub rules: Vec<CssRule>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredPage {
    pub name: String,
    /// The page's site path, such as `/` or `/about`.
    pub path: String,
    /// A file-name-safe stem, `index` for the home page.
    pub file_stem: String,
    /// The page root: a `<main class="page">` holding every block.
    pub root: Element,
}

/// How a target links to one of the site's pages, such as `about.html` or `/about`.
pub type PageLinks<'a> = &'a dyn Fn(&Page) -> String;

pub const VOID_ELEMENTS: &[&str] = &["img", "br", "hr", "input", "meta", "link"];

/// Styles every block type gets on the canvas, independent of the document.
const BASE_RULES: &[(&str, &[(&str, &str)])] = &[
    (
        "body",
        &[
            ("margin", "0"),
            (
                "font-family",
                "system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif",
            ),
            ("color", "var(--color-text, #1f1b2e)"),
            ("background", "var(--color-background, #fff)"),
        ],
    ),
    ("*", &[("box-sizing", "border-box")]),
    (
        ".page",
        &[
            ("max-width", "880px"),
            ("margin", "0 auto"),
            ("padding", "24px 0"),
            ("background", "var(--color-background, #fff)"),
        ],
    ),
    (".blk-heading,\n.blk-text", &[("margin", "0")]),
    (
        ".blk-text",
        &[("line-height", "1.6"), ("white-space", "pre-wrap")],
    ),
    (
        ".blk-button",
        &[
            ("display", "inline-block"),
            ("padding", "10px 20px"),
            ("border-radius", "8px"),
            ("background", "var(--color-accent, #6d4aff)"),
            ("color", "var(--color-on-accent, #fff)"),
            ("text-decoration", "none"),
            ("font-weight", "600"),
        ],
    ),
    (
        ".blk-image",
        &[
            ("max-width", "100%"),
            ("display", "inline-block"),
            ("border-radius", "6px"),
        ],
    ),
    (
        ".blk-vector",
        &[
            ("width", "100%"),
            ("max-width", "480px"),
            ("height", "auto"),
        ],
    ),
    (
        ".blk-nav",
        &[("display", "flex"), ("flex-wrap", "wrap"), ("gap", "20px")],
    ),
    (
        ".blk-nav-link",
        &[
            ("color", "inherit"),
            ("text-decoration", "none"),
            ("font-weight", "600"),
        ],
    ),
    (
        ".blk-nav-link[aria-current=\"page\"]",
        &[("color", "var(--color-accent, #6d4aff)")],
    ),
];

/// Lowers every page, rewriting links to the site's own pages with `links`.
pub fn lower_site(doc: &Document, links: PageLinks) -> LoweredSite {
    let variables = |vars: Vec<(String, String)>| -> Vec<(String, String)> {
        vars.into_iter()
            .filter_map(|(p, v)| declaration(&p, v))
            .collect()
    };
    let mut rules = vec![CssRule {
        media: None,
        selector: ":root".into(),
        declarations: variables(doc.tokens.variables(false)),
    }];
    let dark = variables(doc.tokens.dark_overrides());
    if !dark.is_empty() {
        rules.push(CssRule {
            media: Some("@media (prefers-color-scheme: dark)".into()),
            selector: ":root".into(),
            declarations: dark,
        });
    }
    rules.retain(|r| !r.declarations.is_empty());
    rules.extend(BASE_RULES.iter().map(|(selector, declarations)| {
        CssRule {
            media: None,
            selector: (*selector).to_owned(),
            declarations: declarations
                .iter()
                .map(|(p, v)| ((*p).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }));
    let mut used = HashSet::new();
    let component_names = doc
        .components
        .iter()
        .map(|c| {
            let base = pascal_case(&c.name, "Component");
            let mut name = base.clone();
            let mut n = 2;
            while !used.insert(name.clone()) {
                name = format!("{base}{n}");
                n += 1;
            }
            (c.id.clone(), name)
        })
        .collect();
    let mut cx = Lowering {
        doc,
        links,
        page: &doc.pages[0],
        tree: doc.pages[0].id.clone(),
        template: false,
        expanding: Vec::new(),
        component_names,
        classes: HashMap::new(),
        namer: ClassNamer::default(),
        rules,
        responsive: Breakpoint::ALL.iter().map(|_| Vec::new()).collect(),
    };
    let pages = doc
        .pages
        .iter()
        .map(|page| {
            cx.page = page;
            cx.tree = page.id.clone();
            let children = page
                .nodes
                .iter()
                .map(|node| lower_node(node, &mut cx))
                .collect();
            LoweredPage {
                name: page.name.clone(),
                path: page.path.clone(),
                file_stem: page.file_stem(),
                root: element("main", vec![("class", "page".into())], children),
            }
        })
        .collect();
    let components = doc
        .components
        .iter()
        .map(|c| LoweredComponent {
            name: cx.component_names[&c.id].clone(),
            props: c
                .props
                .iter()
                .map(|p| LoweredProp {
                    name: p.name.clone(),
                    kind: p.kind,
                    default: cx.prop_value(p.kind, &p.default),
                })
                .collect(),
            root: lower_component(c, &c.nodes, true, &mut cx),
        })
        .collect();
    let Lowering {
        mut rules,
        responsive,
        ..
    } = cx;
    rules.extend(responsive.into_iter().flatten());
    LoweredSite {
        pages,
        components,
        rules,
    }
}

/// A PascalCase identifier made from `name`, such as `AboutUs`, prefixed with `prefix` if it
/// would not start with a letter.
pub fn pascal_case(name: &str, prefix: &str) -> String {
    let mut out = String::new();
    for word in name.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    match out.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => out,
        _ => format!("{prefix}{out}"),
    }
}

/// Lowers a component's `nodes` (its own, or an instance's expansion of them) inside a wrapper
/// carrying the component's class. In a `template`, bound fields show their props.
fn lower_component(c: &Component, nodes: &[Node], template: bool, cx: &mut Lowering) -> Element {
    let saved_tree = std::mem::replace(&mut cx.tree, c.id.clone());
    let saved_template = std::mem::replace(&mut cx.template, template);
    cx.expanding.push(c.id.clone());
    let children = nodes.iter().map(|n| lower_node(n, cx)).collect();
    cx.expanding.pop();
    cx.tree = saved_tree;
    cx.template = saved_template;
    let (class_name, _) = cx.class_for(("component".into(), c.id.clone()), &c.name);
    element(
        "div",
        vec![("class", format!("blk-component {class_name}"))],
        children,
    )
}

fn lower_instance(
    component: &str,
    props: &BTreeMap<String, String>,
    cx: &mut Lowering,
) -> OutputNode {
    let Some(c) = cx.doc.component(component) else {
        return OutputNode::Text(String::new());
    };
    // A component that (by hand-editing) contains itself renders nothing the second time.
    if cx.expanding.iter().any(|id| id == component) {
        return OutputNode::Text(String::new());
    }
    let set = c
        .props
        .iter()
        .filter_map(|p| Some((p.name.clone(), cx.prop_value(p.kind, props.get(&p.name)?))))
        .collect();
    let expanded = lower_component(c, &vellum_core::expand(c, props), false, cx);
    OutputNode::Instance(Instance {
        component: cx.component_names[&c.id].clone(),
        props: set,
        expanded,
    })
}

struct Lowering<'a> {
    doc: &'a Document,
    links: PageLinks<'a>,
    /// The page being lowered, for marking its nav link current.
    page: &'a Page,
    /// The page or component whose nodes are being lowered.
    tree: String,
    /// Lowering a component template: bound fields become props, and no nav link is current.
    template: bool,
    /// Components being lowered, innermost last.
    expanding: Vec<String>,
    component_names: HashMap<String, String>,
    /// Each node's class, by tree and node id, so every instance of a component shares its
    /// classes and rules.
    classes: HashMap<(String, String), String>,
    /// Class names are unique across the site, so every page can share one stylesheet.
    namer: ClassNamer,
    rules: Vec<CssRule>,
    /// Override rules for each of [`Breakpoint::ALL`], appended after the base rules.
    responsive: Vec<Vec<CssRule>>,
}

impl Lowering<'_> {
    /// The class for `key`, and whether it was just made.
    fn class_for(&mut self, key: (String, String), label: &str) -> (String, bool) {
        if let Some(name) = self.classes.get(&key) {
            return (name.clone(), false);
        }
        let name = self.namer.name(label);
        self.classes.insert(key, name.clone());
        (name, true)
    }

    /// A prop value as exports may use it: links are sanitized and point at pages, image URLs
    /// are sanitized.
    fn prop_value(&self, kind: PropKind, value: &str) -> String {
        match kind {
            PropKind::Text => value.to_owned(),
            PropKind::Link => self.href(value),
            PropKind::Image => safe_url(value, UrlUse::Image),
        }
    }

    /// A link's target, pointing site paths like `/about#team` at the target's page file.
    fn href(&self, href: &str) -> String {
        let href = href.trim();
        match self.doc.page_for_link(href) {
            Some(page) => {
                let suffix = href.find(['#', '?']).map_or("", |i| &href[i..]);
                format!("{}{suffix}", (self.links)(page))
            }
            None => safe_url(href, UrlUse::Link),
        }
    }
}

fn lower_node(node: &Node, cx: &mut Lowering) -> OutputNode {
    if let NodeKind::Instance { component, props } = &node.kind {
        return lower_instance(component, props, cx);
    }
    let label = match &node.kind {
        // `.svg` would read like an element selector in the stylesheet.
        NodeKind::Svg { .. } => "vector",
        NodeKind::Container { layout, .. } => layout.label(),
        _ => node.kind.type_name(),
    };
    let (class_name, fresh) = cx.class_for((cx.tree.clone(), node.id.clone()), label);
    if fresh {
        push_rules(node, &class_name, cx);
    }

    // In a template, a bound field shows its prop rather than its own value.
    let prop = |field: &str| {
        cx.template
            .then(|| node.bindings.get(field).cloned())
            .flatten()
    };
    let text = |field: &str, value: &str| match prop(field) {
        Some(p) => OutputNode::Prop(p),
        None => OutputNode::Text(value.to_owned()),
    };
    let bind = |mut el: Element, field: &str, attr: &'static str| {
        if let Some(p) = prop(field) {
            el.attrs.retain(|(name, _)| *name != attr);
            el.prop_attrs.push((attr, p));
        }
        el
    };

    // `block` is the element carrying the per-node class; some blocks wrap an inner element.
    let block = |tag, base: &str, attrs: Vec<(&'static str, String)>, children| {
        let mut all = vec![("class", format!("{base} {class_name}"))];
        all.extend(attrs);
        element(tag, all, children)
    };
    let wrap = |inner: Element| {
        element(
            "div",
            vec![("class", class_name.clone())],
            vec![OutputNode::Element(inner)],
        )
    };
    let lowered = match &node.kind {
        NodeKind::Heading { text: value, level } => {
            // The canvas offers H1 to H3 and renders anything deeper as H3.
            let tag = match level {
                1 => "h1",
                2 => "h2",
                _ => "h3",
            };
            block(tag, "blk-heading", vec![], vec![text("text", value)])
        }
        NodeKind::Text { text: value } => block("p", "blk-text", vec![], vec![text("text", value)]),
        NodeKind::Button { label, href } => {
            let a = element(
                "a",
                vec![("class", "blk-button".into()), ("href", cx.href(href))],
                vec![text("label", label)],
            );
            wrap(bind(a, "href", "href"))
        }
        NodeKind::Image { src, alt } => {
            let img = element(
                "img",
                vec![
                    ("class", "blk-image".into()),
                    ("src", safe_url(src, UrlUse::Image)),
                    ("alt", alt.clone()),
                ],
                vec![],
            );
            wrap(bind(bind(img, "src", "src"), "alt", "alt"))
        }
        NodeKind::Spacer { .. } => block(
            "div",
            "blk-spacer",
            vec![("aria-hidden", "true".into())],
            vec![],
        ),
        NodeKind::Svg { drawing } => wrap(lower_drawing(drawing)),
        NodeKind::Container { children, .. } => {
            let children = children.iter().map(|child| lower_node(child, cx)).collect();
            block("div", "blk-container", vec![], children)
        }
        NodeKind::Nav {} => {
            let links = cx
                .doc
                .pages
                .iter()
                .map(|page| {
                    let mut attrs =
                        vec![("class", "blk-nav-link".into()), ("href", (cx.links)(page))];
                    if !cx.template && page.id == cx.page.id {
                        attrs.push(("aria-current", "page".into()));
                    }
                    OutputNode::Element(element(
                        "a",
                        attrs,
                        vec![OutputNode::Text(page.name.clone())],
                    ))
                })
                .collect();
            block("nav", "blk-nav", vec![], links)
        }
        NodeKind::Instance { .. } => unreachable!("handled above"),
    };
    OutputNode::Element(lowered)
}

/// The rule for a node's class, and its breakpoint overrides.
fn push_rules(node: &Node, class_name: &str, cx: &mut Lowering) {
    let declarations = match &node.kind {
        NodeKind::Spacer { height } => vec![
            declaration("height", format!("{}px", fmt(*height))),
            declaration(
                "background",
                node.style
                    .background
                    .as_deref()
                    .map_or("transparent".into(), css::color),
            ),
        ],
        NodeKind::Container { layout, .. } => {
            let mut out = block_declarations(&node.style);
            out.extend(
                layout
                    .declarations()
                    .into_iter()
                    .map(|(p, v)| declaration(p, v)),
            );
            out
        }
        _ => block_declarations(&node.style),
    };
    cx.rules.push(CssRule {
        media: None,
        selector: format!(".{class_name}"),
        declarations: declarations.into_iter().flatten().collect(),
    });
    for (bp, rules) in Breakpoint::ALL.iter().zip(&mut cx.responsive) {
        let Some(o) = node.responsive.get(bp) else {
            continue;
        };
        let declarations: Vec<_> = o
            .declarations(node)
            .into_iter()
            .filter_map(|(p, v)| declaration(p, v))
            .collect();
        if !declarations.is_empty() {
            rules.push(CssRule {
                media: Some(bp.media()),
                selector: format!(".{class_name}"),
                declarations,
            });
        }
    }
}

fn block_declarations(style: &Style) -> Vec<Option<(String, String)>> {
    style
        .declarations()
        .into_iter()
        .map(|(p, v)| declaration(p, v))
        .collect()
}

/// A CSS declaration, or `None` when the value could break out of its rule.
fn declaration(property: &str, value: String) -> Option<(String, String)> {
    let unsafe_value = value.trim().is_empty() || value.contains([';', '{', '}', '<', '>', '\\']);
    (!unsafe_value).then(|| (property.to_owned(), value))
}

fn lower_drawing(drawing: &Drawing) -> Element {
    element(
        "svg",
        vec![
            ("class", "blk-vector".into()),
            ("xmlns", "http://www.w3.org/2000/svg".into()),
            (
                "viewBox",
                format!("0 0 {} {}", fmt(drawing.width), fmt(drawing.height)),
            ),
            ("role", "img".into()),
            ("aria-label", "Vector graphic".into()),
        ],
        drawing
            .shapes
            .iter()
            .map(|shape| OutputNode::Element(lower_shape(shape)))
            .collect(),
    )
}

fn lower_shape(shape: &Shape) -> Element {
    let fill = if shape.geometry.is_filled() {
        shape.style.fill.clone()
    } else {
        "none".into()
    };
    let paint = vec![
        ("fill", fill),
        ("stroke", shape.style.stroke.clone()),
        ("stroke-width", fmt(shape.style.stroke_width)),
    ];
    let (tag, mut attrs) = match &shape.geometry {
        Geometry::Rect { x, y, w, h } => (
            "rect",
            vec![
                ("x", fmt(*x)),
                ("y", fmt(*y)),
                ("width", fmt(*w)),
                ("height", fmt(*h)),
            ],
        ),
        Geometry::Ellipse { cx, cy, rx, ry } => (
            "ellipse",
            vec![
                ("cx", fmt(*cx)),
                ("cy", fmt(*cy)),
                ("rx", fmt(*rx)),
                ("ry", fmt(*ry)),
            ],
        ),
        Geometry::Line { x1, y1, x2, y2 } => (
            "line",
            vec![
                ("x1", fmt(*x1)),
                ("y1", fmt(*y1)),
                ("x2", fmt(*x2)),
                ("y2", fmt(*y2)),
            ],
        ),
        Geometry::Path { nodes, closed } => ("path", vec![("d", path_data(nodes, *closed))]),
    };
    attrs.extend(paint);
    match shape.geometry {
        Geometry::Line { .. } => attrs.push(("stroke-linecap", "round".into())),
        Geometry::Path { .. } => {
            attrs.push(("stroke-linejoin", "round".into()));
            attrs.push(("stroke-linecap", "round".into()));
        }
        _ => {}
    }
    element(tag, attrs, vec![])
}

fn element(
    tag: &'static str,
    attrs: Vec<(&'static str, String)>,
    children: Vec<OutputNode>,
) -> Element {
    Element {
        tag,
        attrs,
        prop_attrs: Vec::new(),
        children,
    }
}

/// Rounds to two decimals and drops a trailing `.0`, matching `vellum-vector`'s SVG output.
pub fn fmt(n: f64) -> String {
    css::number(n)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlUse {
    Link,
    Image,
}

/// Keeps relative and http(s)/mailto/tel URLs; replaces script-capable ones.
pub fn safe_url(url: &str, use_: UrlUse) -> String {
    let trimmed = url.trim();
    // Browsers ignore embedded whitespace and control characters when parsing the scheme.
    let compact: String = trimmed
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    let has_scheme = compact
        .find(':')
        .is_some_and(|end| !compact[..end].contains(['/', '?', '#']));
    if !has_scheme {
        return trimmed.to_owned();
    }
    let lower = trimmed.to_ascii_lowercase();
    let safe = ["http:", "https:", "mailto:", "tel:"]
        .iter()
        .any(|s| lower.starts_with(s));
    let safe_image = use_ == UrlUse::Image
        && ["png", "jpeg", "jpg", "gif", "webp", "avif"]
            .iter()
            .any(|kind| {
                lower.starts_with(&format!("data:image/{kind};"))
                    || lower.starts_with(&format!("data:image/{kind},"))
            });
    match (safe || safe_image, use_) {
        (true, _) => trimmed.to_owned(),
        (false, UrlUse::Link) => "#".into(),
        (false, UrlUse::Image) => String::new(),
    }
}

/// Produces unique, valid CSS class names from labels.
#[derive(Debug, Default)]
pub struct ClassNamer {
    used: HashSet<String>,
}

impl ClassNamer {
    pub fn name(&mut self, label: &str) -> String {
        let mut base = String::new();
        for c in label.to_lowercase().chars() {
            if c.is_ascii_alphanumeric() {
                base.push(c);
            } else if !base.is_empty() && !base.ends_with('-') {
                base.push('-');
            }
        }
        let base = base.trim_end_matches('-');
        let base = match base.chars().next() {
            Some(c) if c.is_ascii_alphabetic() => base.to_owned(),
            Some(_) => format!("n-{base}"),
            None => "n".to_owned(),
        };
        let mut name = base.clone();
        let mut n = 2;
        while self.used.contains(&name) {
            name = format!("{base}-{n}");
            n += 1;
        }
        self.used.insert(name.clone());
        name
    }
}

/// Serializes CSS rules with two-space indentation. Neighboring rules under the same at-rule
/// share one block.
pub fn print_css(rules: &[CssRule]) -> String {
    let print_rule = |rule: &CssRule| {
        let body: String = rule
            .declarations
            .iter()
            .map(|(p, v)| format!("  {p}: {v};\n"))
            .collect();
        format!("{} {{\n{body}}}\n", rule.selector)
    };
    rules
        .chunk_by(|a, b| a.media.is_some() && a.media == b.media)
        .map(|group| match &group[0].media {
            None => group.iter().map(print_rule).collect::<Vec<_>>().join("\n"),
            Some(media) => {
                let inner: String = group
                    .iter()
                    .map(print_rule)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .lines()
                    .map(|l| match l {
                        "" => "\n".to_owned(),
                        l => format!("  {l}\n"),
                    })
                    .collect();
                format!("{media} {{\n{inner}}}\n")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use vellum_schema::{Component, Override};

    fn html_links(page: &Page) -> String {
        format!("{}.html", page.file_stem())
    }

    fn two_pages() -> Document {
        let mut doc = Document::default();
        doc.pages.push(Page {
            id: "about".into(),
            name: "About".into(),
            path: "/about".into(),
            nodes: vec![],
        });
        for page in &mut doc.pages {
            page.nodes = vec![
                Node::new("nav", NodeKind::Nav {}),
                Node::new(
                    "b",
                    NodeKind::Button {
                        label: "Go".into(),
                        href: "/about/#team".into(),
                    },
                ),
            ];
        }
        doc
    }

    fn attr<'a>(el: &'a Element, name: &str) -> Option<&'a str> {
        el.attrs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    fn child(el: &Element, i: usize) -> &Element {
        match &el.children[i] {
            OutputNode::Element(e) => e,
            OutputNode::Instance(i) => &i.expanded,
            _ => panic!("expected an element"),
        }
    }

    /// `two_pages`, with the nav and button of each page made into one component: a "Site
    /// header" whose button label and link are props. Home sets the label; About keeps the
    /// defaults.
    fn with_header() -> Document {
        use std::collections::BTreeMap;
        use vellum_schema::{PropDef, PropKind};
        let mut doc = two_pages();
        let mut button = doc.pages[0].nodes[1].clone();
        button.bindings = BTreeMap::from([
            ("label".into(), "cta".into()),
            ("href".into(), "target".into()),
        ]);
        doc.components.push(Component {
            id: "hdr".into(),
            name: "Site header".into(),
            props: vec![
                PropDef {
                    name: "cta".into(),
                    kind: PropKind::Text,
                    default: "Read more".into(),
                },
                PropDef {
                    name: "target".into(),
                    kind: PropKind::Link,
                    default: "javascript:alert(1)".into(),
                },
            ],
            nodes: vec![doc.pages[0].nodes[0].clone(), button],
        });
        for (i, page) in doc.pages.iter_mut().enumerate() {
            let props = match i {
                0 => BTreeMap::from([
                    ("cta".into(), "Start".into()),
                    ("target".into(), "/about".into()),
                ]),
                _ => BTreeMap::new(),
            };
            page.nodes = vec![Node::new(
                format!("i{i}"),
                NodeKind::Instance {
                    component: "hdr".into(),
                    props,
                },
            )];
        }
        doc
    }

    #[test]
    fn instances_share_classes_and_fill_in_props() {
        let site = lower_site(&with_header(), &html_links);
        let OutputNode::Instance(home) = &site.pages[0].root.children[0] else {
            panic!("instance");
        };
        let OutputNode::Instance(about) = &site.pages[1].root.children[0] else {
            panic!("instance");
        };
        assert_eq!(home.component, "SiteHeader");
        assert_eq!(
            home.props,
            [
                ("cta".to_owned(), "Start".to_owned()),
                ("target".to_owned(), "about.html".to_owned())
            ]
        );
        assert!(about.props.is_empty());
        assert_eq!(
            attr(&home.expanded, "class"),
            Some("blk-component site-header")
        );
        assert_eq!(
            attr(&about.expanded, "class"),
            attr(&home.expanded, "class")
        );
        let button = |e: &Element| child(child(e, 1), 0).clone();
        assert_eq!(attr(&button(&home.expanded), "href"), Some("about.html"));
        assert_eq!(
            button(&home.expanded).children,
            [OutputNode::Text("Start".into())]
        );
        assert_eq!(
            attr(&button(&about.expanded), "href"),
            Some("#"),
            "sanitized default"
        );
        // The nav still marks the page it's on.
        let current = |e: &Element, i| attr(child(child(e, 0), i), "aria-current").is_some();
        assert!(current(&home.expanded, 0) && current(&about.expanded, 1));
        // One rule per component node, not per instance.
        let css = print_css(&site.rules);
        assert_eq!(css.matches(".button {").count(), 1);
        assert!(!css.contains(".button-2"));
    }

    #[test]
    fn component_templates_show_their_props() {
        let site = lower_site(&with_header(), &html_links);
        let header = &site.components[0];
        assert_eq!(header.name, "SiteHeader");
        assert_eq!(header.props[1].default, "#");
        let a = child(child(&header.root, 1), 0);
        assert_eq!(a.children, [OutputNode::Prop("cta".into())]);
        assert_eq!(a.prop_attrs, [("href", "target".to_owned())]);
        assert_eq!(attr(a, "href"), None);
        let nav = child(&header.root, 0);
        assert!((0..2).all(|i| attr(child(nav, i), "aria-current").is_none()));
    }

    #[test]
    fn a_component_inside_itself_renders_once() {
        let mut doc = with_header();
        let inner = doc.pages[1].nodes[0].clone();
        doc.components[0].nodes.push(inner);
        let site = lower_site(&doc, &html_links);
        let OutputNode::Instance(home) = &site.pages[0].root.children[0] else {
            panic!("instance");
        };
        assert_eq!(home.expanded.children[2], OutputNode::Text(String::new()));
    }

    #[test]
    fn links_pages_and_marks_the_current_one() {
        let site = lower_site(&two_pages(), &html_links);
        assert_eq!(site.pages.len(), 2);
        assert_eq!(site.pages[1].file_stem, "about");
        let about_nav = child(&site.pages[1].root, 0);
        assert_eq!(about_nav.tag, "nav");
        let links: Vec<_> = (0..2).map(|i| child(about_nav, i)).collect();
        assert_eq!(attr(links[0], "href"), Some("index.html"));
        assert_eq!(attr(links[0], "aria-current"), None);
        assert_eq!(attr(links[1], "aria-current"), Some("page"));
        let button = child(child(&site.pages[0].root, 1), 0);
        assert_eq!(attr(button, "href"), Some("about.html#team"));
    }

    #[test]
    fn class_names_stay_unique_across_pages() {
        let site = lower_site(&two_pages(), &html_links);
        let class = |p: usize| attr(child(&site.pages[p].root, 0), "class").map(str::to_owned);
        assert_eq!(class(0).as_deref(), Some("blk-nav nav"));
        assert_eq!(class(1).as_deref(), Some("blk-nav nav-2"));
    }

    #[test]
    fn overrides_become_media_rules_after_the_base_rules() {
        let mut doc = two_pages();
        let nav = &mut doc.pages[0].nodes[0];
        nav.responsive.insert(
            Breakpoint::Mobile,
            Override {
                hidden: true,
                ..Override::default()
            },
        );
        nav.responsive.insert(
            Breakpoint::Tablet,
            Override {
                align: Some(vellum_schema::Align::Center),
                ..Override::default()
            },
        );
        let css = print_css(&lower_site(&doc, &html_links).rules);
        let tablet = css
            .find("@media (max-width: 900px) {\n  .nav {\n    text-align: center;")
            .expect("tablet rule");
        let mobile = css
            .find("@media (max-width: 600px) {\n  .nav {\n    display: none;")
            .expect("mobile rule");
        let last_base = css.rfind(".button-2 {").expect("base rule");
        assert!(last_base < tablet && tablet < mobile);
    }

    #[test]
    fn neutralizes_script_urls() {
        assert_eq!(safe_url("javascript:alert(1)", UrlUse::Link), "#");
        assert_eq!(safe_url(" java\tscript:alert(1)", UrlUse::Link), "#");
        assert_eq!(safe_url("JavaScript:alert(1)", UrlUse::Link), "#");
        assert_eq!(safe_url("data:text/html,<script>", UrlUse::Image), "");
        assert_eq!(
            safe_url("data:image/png;base64,AAAA", UrlUse::Image),
            "data:image/png;base64,AAAA"
        );
        assert_eq!(safe_url("data:image/png;base64,AAAA", UrlUse::Link), "#");
        assert_eq!(safe_url("/about#team", UrlUse::Link), "/about#team");
        assert_eq!(safe_url("/a:b", UrlUse::Link), "/a:b");
        assert_eq!(
            safe_url("mailto:hi@example.com", UrlUse::Link),
            "mailto:hi@example.com"
        );
    }

    #[test]
    fn drops_style_values_that_could_escape_a_rule() {
        let style = Style {
            color: "red; } body { display: none".into(),
            background: Some("#fff".into()),
            padding: vellum_schema::Length::Px(16.0),
            ..Style::default()
        };
        let declarations: Vec<_> = block_declarations(&style).into_iter().flatten().collect();
        assert_eq!(
            declarations,
            [
                ("text-align".to_owned(), "left".to_owned()),
                ("background".to_owned(), "#fff".to_owned()),
                ("padding".to_owned(), "16px".to_owned()),
            ]
        );
    }

    #[test]
    fn drops_token_values_that_could_escape_a_rule() {
        let mut doc = Document::default();
        doc.tokens.colors[0].light = "red; } body { display: none".into();
        doc.tokens.fonts[0].family = "x</style><script>".into();
        let site = lower_site(&doc, &html_links);
        let css = print_css(&site.rules);
        assert!(!css.contains("display: none"));
        assert!(!css.contains("<script>"));
        assert!(css.contains("--color-muted: #6b6780;"));
    }

    #[test]
    fn nests_containers_and_wraps_dark_overrides_in_a_media_query() {
        let mut doc = Document::default();
        use vellum_schema::Layout;
        let mut row = Node::new(
            "r",
            NodeKind::Container {
                layout: Layout::row(),
                children: vec![],
            },
        );
        row.children_mut()
            .unwrap()
            .push(Node::new("t", NodeKind::Text { text: "hi".into() }));
        doc.pages[0].nodes.push(row);
        let site = lower_site(&doc, &html_links);
        let page = &site.pages[0];
        let OutputNode::Element(row) = &page.root.children[0] else {
            panic!("row element");
        };
        assert_eq!(row.attrs[0], ("class", "blk-container row".into()));
        assert!(matches!(&row.children[0], OutputNode::Element(e) if e.tag == "p"));
        let css = print_css(&site.rules);
        assert!(css.contains(
            "@media (prefers-color-scheme: dark) {\n  :root {\n    --color-text: #f4f3f8;"
        ));
        assert!(css.contains(".row {\n  text-align: left;"));
        assert!(css.contains("  display: flex;\n  flex-direction: row;"));
    }

    #[test]
    fn keeps_class_names_unique() {
        let mut namer = ClassNamer::default();
        let names: Vec<_> = ["Card", "Card", "card 2", "1st", "!!!", "Hero  Banner"]
            .iter()
            .map(|l| namer.name(l))
            .collect();
        assert_eq!(
            names,
            ["card", "card-2", "card-2-2", "n-1st", "n", "hero-banner"]
        );
    }

    #[test]
    fn formats_numbers_like_the_vector_crate() {
        assert_eq!(fmt(16.0), "16");
        assert_eq!(fmt(1.005), "1");
        assert_eq!(fmt(2.346), "2.35");
        assert_eq!(fmt(-0.001), "0");
    }
}
