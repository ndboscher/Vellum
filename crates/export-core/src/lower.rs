//! Lowers a [`Document`] into a target-neutral element tree plus CSS rules.
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

use std::collections::HashSet;

use vellum_schema::{css, Document, Drawing, Geometry, Node, NodeKind, Shape, Style};
use vellum_vector::path_data;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputNode {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub tag: &'static str,
    /// Attributes in HTML spelling (`class`, `stroke-width`, `viewBox`).
    pub attrs: Vec<(&'static str, String)>,
    pub children: Vec<OutputNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CssRule {
    /// An enclosing at-rule such as `@media (prefers-color-scheme: dark)`.
    pub media: Option<String>,
    pub selector: String,
    pub declarations: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredPage {
    /// The page root: a `<main class="page">` holding every block.
    pub root: Element,
    /// Design tokens as custom properties, base rules shared by every export, then one rule
    /// per node.
    pub rules: Vec<CssRule>,
}

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
];

pub fn lower_document(doc: &Document) -> LoweredPage {
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
    let mut namer = ClassNamer::default();
    let children = doc
        .nodes
        .iter()
        .map(|node| OutputNode::Element(lower_node(node, &mut namer, &mut rules)))
        .collect();
    LoweredPage {
        root: element("main", vec![("class", "page".into())], children),
        rules,
    }
}

fn lower_node(node: &Node, namer: &mut ClassNamer, rules: &mut Vec<CssRule>) -> Element {
    let label = match &node.kind {
        // `.svg` would read like an element selector in the stylesheet.
        NodeKind::Svg { .. } => "vector",
        NodeKind::Container { layout, .. } => layout.label(),
        _ => node.kind.type_name(),
    };
    let class_name = namer.name(label);
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
    rules.push(CssRule {
        media: None,
        selector: format!(".{class_name}"),
        declarations: declarations.into_iter().flatten().collect(),
    });

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
    match &node.kind {
        NodeKind::Heading { text, level } => {
            // The canvas offers H1 to H3 and renders anything deeper as H3.
            let tag = match level {
                1 => "h1",
                2 => "h2",
                _ => "h3",
            };
            block(
                tag,
                "blk-heading",
                vec![],
                vec![OutputNode::Text(text.clone())],
            )
        }
        NodeKind::Text { text } => block(
            "p",
            "blk-text",
            vec![],
            vec![OutputNode::Text(text.clone())],
        ),
        NodeKind::Button { label, href } => wrap(element(
            "a",
            vec![
                ("class", "blk-button".into()),
                ("href", safe_url(href, UrlUse::Link)),
            ],
            vec![OutputNode::Text(label.clone())],
        )),
        NodeKind::Image { src, alt } => wrap(element(
            "img",
            vec![
                ("class", "blk-image".into()),
                ("src", safe_url(src, UrlUse::Image)),
                ("alt", alt.clone()),
            ],
            vec![],
        )),
        NodeKind::Spacer { .. } => block(
            "div",
            "blk-spacer",
            vec![("aria-hidden", "true".into())],
            vec![],
        ),
        NodeKind::Svg { drawing } => wrap(lower_drawing(drawing)),
        NodeKind::Container { children, .. } => {
            let children = children
                .iter()
                .map(|child| OutputNode::Element(lower_node(child, namer, rules)))
                .collect();
            block("div", "blk-container", vec![], children)
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

/// Serializes CSS rules with two-space indentation.
pub fn print_css(rules: &[CssRule]) -> String {
    rules
        .iter()
        .map(|rule| {
            let body: String = rule
                .declarations
                .iter()
                .map(|(p, v)| format!("  {p}: {v};\n"))
                .collect();
            let rule_text = format!("{} {{\n{body}}}\n", rule.selector);
            match &rule.media {
                None => rule_text,
                Some(media) => {
                    let inner: String = rule_text.lines().map(|l| format!("  {l}\n")).collect();
                    format!("{media} {{\n{inner}}}\n")
                }
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let page = lower_document(&doc);
        let css = print_css(&page.rules);
        assert!(!css.contains("display: none"));
        assert!(!css.contains("<script>"));
        assert!(css.contains("--color-muted: #6b6780;"));
    }

    #[test]
    fn nests_containers_and_wraps_dark_overrides_in_a_media_query() {
        let mut doc = Document::default();
        use vellum_schema::Layout;
        let mut row = Node {
            id: "r".into(),
            style: Style::default(),
            kind: NodeKind::Container {
                layout: Layout::row(),
                children: vec![],
            },
        };
        row.children_mut().unwrap().push(Node {
            id: "t".into(),
            style: Style::default(),
            kind: NodeKind::Text { text: "hi".into() },
        });
        doc.nodes.push(row);
        let page = lower_document(&doc);
        let OutputNode::Element(row) = &page.root.children[0] else {
            panic!("row element");
        };
        assert_eq!(row.attrs[0], ("class", "blk-container row".into()));
        assert!(matches!(&row.children[0], OutputNode::Element(e) if e.tag == "p"));
        let css = print_css(&page.rules);
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
