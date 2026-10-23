//! The Vellum page document.
//!
//! Everything else in the workspace reads and writes these types: the editor
//! mutates them through `vellum-core`, the vector tools build [`Shape`]s, the
//! exporters turn a [`Document`] into code and the server stores it as JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bumped whenever the serialized shape changes, so old files can be migrated.
///
/// Version 2 added design [`Tokens`] and [`NodeKind::Container`]. Version 3 made the document a
/// site of [`Page`]s and added per-breakpoint [`Override`]s and [`NodeKind::Nav`]. Version 4
/// added reusable [`Component`]s, and version 5 keyframe [`Animation`]s on blocks and shapes.
/// Older files still load: they get the default tokens, and their block list becomes the home
/// page.
pub const SCHEMA_VERSION: u32 = 5;

pub type NodeId = String;

/// A site: shared tokens and one or more pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "DocumentRepr")]
pub struct Document {
    pub version: u32,
    pub title: String,
    pub tokens: Tokens,
    /// In navigation order. The first page is the home page, at `/`.
    pub pages: Vec<Page>,
    /// Reusable blocks, placed on pages as [`NodeKind::Instance`]s.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<Component>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            title: "Untitled site".into(),
            tokens: Tokens::default(),
            pages: vec![Page::home(Vec::new())],
            components: Vec::new(),
        }
    }
}

impl Document {
    pub fn component(&self, id: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    pub fn component_mut(&mut self, id: &str) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.id == id)
    }

    /// The node tree of the page or component with this id.
    pub fn tree(&self, id: &str) -> Option<&Vec<Node>> {
        self.page(id)
            .map(|p| &p.nodes)
            .or_else(|| self.component(id).map(|c| &c.nodes))
    }

    pub fn tree_mut(&mut self, id: &str) -> Option<&mut Vec<Node>> {
        if self.page(id).is_some() {
            return self.page_mut(id).map(|p| &mut p.nodes);
        }
        self.component_mut(id).map(|c| &mut c.nodes)
    }

    /// Every node tree: each page's, then each component's.
    pub fn trees_mut(&mut self) -> impl Iterator<Item = &mut Vec<Node>> {
        let pages = self.pages.iter_mut().map(|p| &mut p.nodes);
        pages.chain(self.components.iter_mut().map(|c| &mut c.nodes))
    }

    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }

    pub fn page_mut(&mut self, id: &str) -> Option<&mut Page> {
        self.pages.iter_mut().find(|p| p.id == id)
    }

    /// The page with this id, or the home page if it is gone (say, after an undo).
    pub fn page_or_home(&self, id: &str) -> &Page {
        self.page(id)
            .or(self.pages.first())
            .expect("a document always has a page")
    }

    pub fn page_or_home_mut(&mut self, id: &str) -> &mut Page {
        let i = self.pages.iter().position(|p| p.id == id).unwrap_or(0);
        &mut self.pages[i]
    }

    /// The page a site-relative link like `/about#team` points at, if any.
    pub fn page_for_link(&self, href: &str) -> Option<&Page> {
        let path = href.split(['#', '?']).next()?;
        if !path.starts_with('/') {
            return None;
        }
        let path = match path.trim_end_matches('/') {
            "" => "/",
            p => p,
        };
        self.pages.iter().find(|p| p.path == path)
    }
}

/// Reads every schema version: versions 1 and 2 had one page's `nodes` at the top level.
#[derive(Deserialize)]
struct DocumentRepr {
    version: u32,
    title: String,
    #[serde(default)]
    tokens: Tokens,
    #[serde(default)]
    pages: Vec<Page>,
    #[serde(default)]
    nodes: Option<Vec<Node>>,
    #[serde(default)]
    components: Vec<Component>,
}

impl From<DocumentRepr> for Document {
    fn from(repr: DocumentRepr) -> Self {
        let mut pages = repr.pages;
        if pages.is_empty() {
            pages.push(Page::home(repr.nodes.unwrap_or_default()));
        }
        Self {
            version: repr.version.max(SCHEMA_VERSION),
            title: repr.title,
            tokens: repr.tokens,
            pages,
            components: repr.components,
        }
    }
}

/// One page of the site.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: NodeId,
    /// Shown in navigation and the browser tab.
    pub name: String,
    /// `/` for the home page, otherwise `/` and a slug, such as `/about`.
    pub path: String,
    /// The page's top level, stacked vertically. Containers hold further nodes.
    pub nodes: Vec<Node>,
}

/// A reusable group of blocks. Its props are values each instance can set, shown by the
/// content fields bound to them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: NodeId,
    pub name: String,
    #[serde(default)]
    pub props: Vec<PropDef>,
    pub nodes: Vec<Node>,
}

impl Component {
    pub fn prop(&self, name: &str) -> Option<&PropDef> {
        self.props.iter().find(|p| p.name == name)
    }
}

/// One prop of a component. Its name is a JavaScript identifier, since React and Vue exports
/// use it as one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropDef {
    pub name: String,
    pub kind: PropKind,
    pub default: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PropKind {
    Text,
    /// A URL a visitor follows; exports sanitize it and point site paths at pages.
    Link,
    /// An image URL.
    Image,
}

impl PropKind {
    pub const ALL: [PropKind; 3] = [PropKind::Text, PropKind::Link, PropKind::Image];

    pub fn as_str(self) -> &'static str {
        match self {
            PropKind::Text => "text",
            PropKind::Link => "link",
            PropKind::Image => "image",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

impl Page {
    pub fn home(nodes: Vec<Node>) -> Self {
        Self {
            id: "home".into(),
            name: "Home".into(),
            path: "/".into(),
            nodes,
        }
    }

    pub fn is_home(&self) -> bool {
        self.path == "/"
    }

    /// A file-name-safe stem for this page: `index` for the home page, else its slug.
    pub fn file_stem(&self) -> String {
        match css::slug(&self.path) {
            s if s.is_empty() => "index".into(),
            s => s,
        }
    }
}

/// One block on the page, or a container of blocks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    #[serde(default)]
    pub style: Style,
    #[serde(flatten)]
    pub kind: NodeKind,
    /// Style changes below each breakpoint's width.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub responsive: BTreeMap<Breakpoint, Override>,
    /// Inside a component: content fields (see [`NodeKind::bindable_fields`]) that show a prop's
    /// value instead of their own, as field name to prop name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,
    /// Played when the page loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<Animation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NodeKind {
    Heading {
        text: String,
        level: u8,
    },
    Text {
        text: String,
    },
    Button {
        label: String,
        href: String,
    },
    Image {
        src: String,
        alt: String,
    },
    Spacer {
        height: f64,
    },
    Svg {
        drawing: Drawing,
    },
    /// Lays out its children with flexbox or grid.
    Container {
        layout: Layout,
        children: Vec<Node>,
    },
    /// Links to every page of the site, in page order.
    Nav {},
    /// A placement of a [`Component`], with values for its props. Missing props use the
    /// prop's default.
    Instance {
        component: NodeId,
        #[serde(default)]
        props: BTreeMap<String, String>,
    },
}

impl Node {
    pub fn new(id: impl Into<NodeId>, kind: NodeKind) -> Self {
        Self {
            id: id.into(),
            style: Style::default(),
            kind,
            responsive: BTreeMap::new(),
            bindings: BTreeMap::new(),
            animation: None,
        }
    }

    /// The nested nodes, if this is a container.
    pub fn children(&self) -> Option<&Vec<Node>> {
        match &self.kind {
            NodeKind::Container { children, .. } => Some(children),
            _ => None,
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match &mut self.kind {
            NodeKind::Container { children, .. } => Some(children),
            _ => None,
        }
    }
}

impl NodeKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            NodeKind::Heading { .. } => "heading",
            NodeKind::Text { .. } => "text",
            NodeKind::Button { .. } => "button",
            NodeKind::Image { .. } => "image",
            NodeKind::Spacer { .. } => "spacer",
            NodeKind::Svg { .. } => "svg",
            NodeKind::Container { .. } => "container",
            NodeKind::Nav {} => "nav",
            NodeKind::Instance { .. } => "instance",
        }
    }

    /// The content fields a component can bind to a prop, with the kind of value each holds.
    pub fn bindable_fields(&self) -> &'static [(&'static str, PropKind)] {
        match self {
            NodeKind::Heading { .. } | NodeKind::Text { .. } => &[("text", PropKind::Text)],
            NodeKind::Button { .. } => &[("label", PropKind::Text), ("href", PropKind::Link)],
            NodeKind::Image { .. } => &[("src", PropKind::Image), ("alt", PropKind::Text)],
            _ => &[],
        }
    }

    /// The value of a bindable field.
    pub fn field(&self, name: &str) -> Option<&str> {
        match (self, name) {
            (NodeKind::Heading { text, .. } | NodeKind::Text { text }, "text") => Some(text),
            (NodeKind::Button { label, .. }, "label") => Some(label),
            (NodeKind::Button { href, .. }, "href") => Some(href),
            (NodeKind::Image { src, .. }, "src") => Some(src),
            (NodeKind::Image { alt, .. }, "alt") => Some(alt),
            _ => None,
        }
    }

    pub fn set_field(&mut self, name: &str, value: String) {
        let slot = match (self, name) {
            (NodeKind::Heading { text, .. } | NodeKind::Text { text }, "text") => text,
            (NodeKind::Button { label, .. }, "label") => label,
            (NodeKind::Button { href, .. }, "href") => href,
            (NodeKind::Image { src, .. }, "src") => src,
            (NodeKind::Image { alt, .. }, "alt") => alt,
            _ => return,
        };
        *slot = value;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub fn as_css(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

/// Block styling, kept to a strict subset of CSS so every export target can express it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub align: Align,
    /// CSS color, or a color token reference such as `{text}`.
    pub color: String,
    /// CSS color or color token reference, or `None` for transparent.
    pub background: Option<String>,
    /// Padding on all sides.
    pub padding: Length,
    /// Name of a type token, or `None` to inherit the page font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            align: Align::Left,
            color: token_ref("text"),
            background: None,
            padding: Length::Token("md".into()),
            font: None,
        }
    }
}

impl Style {
    /// The CSS declarations for a block with this style. The editor canvas and every exporter
    /// use this list, so a block looks the same in both.
    pub fn declarations(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![
            ("text-align", self.align.as_css().to_owned()),
            ("color", css::color(&self.color)),
        ];
        if let Some(background) = &self.background {
            out.push(("background", css::color(background)));
        }
        out.push(("padding", self.padding.to_css()));
        if let Some(font) = &self.font {
            out.push(("font", css::var("font", font)));
        }
        out
    }
}

/// A size: a literal number of px, or a spacing token reference written `"{name}"` in JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "LengthRepr", into = "LengthRepr")]
pub enum Length {
    Px(f64),
    /// The name of a spacing token.
    Token(String),
}

impl Length {
    pub fn to_css(&self) -> String {
        match self {
            Length::Px(px) => format!("{}px", css::number(*px)),
            Length::Token(name) => css::var("space", name),
        }
    }
}

impl Default for Length {
    fn default() -> Self {
        Length::Px(0.0)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum LengthRepr {
    Px(f64),
    Ref(String),
}

impl From<Length> for LengthRepr {
    fn from(length: Length) -> Self {
        match length {
            Length::Px(px) => LengthRepr::Px(px),
            Length::Token(name) => LengthRepr::Ref(token_ref(&name)),
        }
    }
}

impl TryFrom<LengthRepr> for Length {
    type Error = String;

    fn try_from(repr: LengthRepr) -> Result<Self, Self::Error> {
        match repr {
            LengthRepr::Px(px) => Ok(Length::Px(px)),
            LengthRepr::Ref(s) => parse_token_ref(&s)
                .map(|name| Length::Token(name.to_owned()))
                .ok_or_else(|| {
                    format!("expected a number or a token reference like \"{{md}}\", got \"{s}\"")
                }),
        }
    }
}

/// Writes a token reference: `token_ref("accent")` is `"{accent}"`.
pub fn token_ref(name: &str) -> String {
    format!("{{{name}}}")
}

/// The token name in a reference like `{accent}`, or `None` for a literal value.
pub fn parse_token_ref(value: &str) -> Option<&str> {
    value
        .trim()
        .strip_prefix('{')?
        .strip_suffix('}')
        .filter(|name| !name.is_empty())
}

/// Design tokens: named colors (with an optional dark-mode value), type styles and spacing.
/// Styles reference them by name, and exports turn them into CSS custom properties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub colors: Vec<ColorToken>,
    pub fonts: Vec<FontToken>,
    pub spacing: Vec<SpaceToken>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorToken {
    pub name: String,
    pub light: String,
    /// Used when the visitor prefers a dark color scheme; `None` keeps `light`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
}

/// A type style: family, size in px, weight and unitless line height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontToken {
    pub name: String,
    pub family: String,
    pub size: f64,
    pub weight: u16,
    pub line_height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpaceToken {
    pub name: String,
    /// In px.
    pub value: f64,
}

/// Color tokens the base page and button styles read, so they can be edited but not
/// renamed or removed.
pub const BASE_COLOR_TOKENS: &[&str] = &["text", "background", "accent", "on-accent"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Color,
    Font,
    Space,
}

impl TokenKind {
    /// The prefix of the token's CSS custom property, e.g. `--color-accent`.
    pub fn css_prefix(self) -> &'static str {
        match self {
            TokenKind::Color => "color",
            TokenKind::Font => "font",
            TokenKind::Space => "space",
        }
    }

    /// Whether the token is one of the [`BASE_COLOR_TOKENS`].
    pub fn is_base(self, name: &str) -> bool {
        self == TokenKind::Color && BASE_COLOR_TOKENS.contains(&name)
    }
}

const SYSTEM_FONT: &str = "system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif";

impl Default for Tokens {
    fn default() -> Self {
        let color = |name: &str, light: &str, dark: &str| ColorToken {
            name: name.into(),
            light: light.into(),
            dark: Some(dark.into()),
        };
        let font = |name: &str, size, weight, line_height| FontToken {
            name: name.into(),
            family: SYSTEM_FONT.into(),
            size,
            weight,
            line_height,
        };
        let space = |name: &str, value| SpaceToken {
            name: name.into(),
            value,
        };
        Self {
            colors: vec![
                color("text", "#1f1b2e", "#f4f3f8"),
                color("muted", "#6b6780", "#a7a3b8"),
                color("background", "#ffffff", "#15131c"),
                color("surface", "#f4f3f8", "#221f2c"),
                color("accent", "#6d4aff", "#9d85ff"),
                color("on-accent", "#ffffff", "#15131c"),
            ],
            fonts: vec![
                font("display", 48.0, 800, 1.1),
                font("heading", 28.0, 700, 1.25),
                font("body", 16.0, 400, 1.6),
                font("small", 13.0, 400, 1.5),
            ],
            spacing: vec![
                space("xs", 4.0),
                space("sm", 8.0),
                space("md", 16.0),
                space("lg", 24.0),
                space("xl", 48.0),
            ],
        }
    }
}

impl Tokens {
    pub fn names(&self, kind: TokenKind) -> Vec<&str> {
        match kind {
            TokenKind::Color => self.colors.iter().map(|t| t.name.as_str()).collect(),
            TokenKind::Font => self.fonts.iter().map(|t| t.name.as_str()).collect(),
            TokenKind::Space => self.spacing.iter().map(|t| t.name.as_str()).collect(),
        }
    }

    /// Custom property declarations for every token, with dark color values when `dark`.
    pub fn variables(&self, dark: bool) -> Vec<(String, String)> {
        let colors = self.colors.iter().map(|t| {
            let value = match (&t.dark, dark) {
                (Some(d), true) => d,
                _ => &t.light,
            };
            (css::property("color", &t.name), value.clone())
        });
        let fonts = self.fonts.iter().map(|t| {
            (
                css::property("font", &t.name),
                format!(
                    "{} {}px/{} {}",
                    t.weight,
                    css::number(t.size),
                    css::number(t.line_height),
                    t.family
                ),
            )
        });
        let spacing = self.spacing.iter().map(|t| {
            (
                css::property("space", &t.name),
                format!("{}px", css::number(t.value)),
            )
        });
        colors.chain(fonts).chain(spacing).collect()
    }

    /// Only the color properties whose dark value differs, for a `prefers-color-scheme` rule.
    pub fn dark_overrides(&self) -> Vec<(String, String)> {
        self.colors
            .iter()
            .filter_map(|t| {
                let dark = t.dark.as_ref().filter(|d| **d != t.light)?;
                Some((css::property("color", &t.name), dark.clone()))
            })
            .collect()
    }
}

/// How a container arranges its children.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub mode: LayoutMode,
    /// Flex only.
    pub direction: Direction,
    /// Flex only: let children flow onto more lines.
    pub wrap: bool,
    /// Grid only: equal-width columns.
    pub columns: u8,
    pub gap: Length,
    /// Cross-axis alignment of the children.
    pub align: AlignItems,
    /// Flex only: main-axis distribution.
    pub justify: Justify,
}

impl Layout {
    pub fn stack() -> Self {
        Self {
            mode: LayoutMode::Flex,
            direction: Direction::Column,
            wrap: false,
            columns: 2,
            gap: Length::Token("md".into()),
            align: AlignItems::Stretch,
            justify: Justify::Start,
        }
    }

    pub fn row() -> Self {
        Self {
            direction: Direction::Row,
            wrap: true,
            align: AlignItems::Center,
            ..Self::stack()
        }
    }

    pub fn grid(columns: u8) -> Self {
        Self {
            mode: LayoutMode::Grid,
            columns,
            ..Self::stack()
        }
    }

    /// What the palette calls this layout: `stack`, `row` or `grid`.
    pub fn label(&self) -> &'static str {
        match (self.mode, self.direction) {
            (LayoutMode::Grid, _) => "grid",
            (LayoutMode::Flex, Direction::Row) => "row",
            (LayoutMode::Flex, Direction::Column) => "stack",
        }
    }

    /// Whether children run left to right, so drop targets split on x rather than y.
    pub fn is_horizontal(&self) -> bool {
        self.mode == LayoutMode::Grid || self.direction == Direction::Row
    }

    pub fn declarations(&self) -> Vec<(&'static str, String)> {
        let mut out = match self.mode {
            LayoutMode::Flex => {
                let mut flex = vec![
                    ("display", "flex".to_owned()),
                    ("flex-direction", self.direction.as_css().to_owned()),
                ];
                if self.wrap {
                    flex.push(("flex-wrap", "wrap".to_owned()));
                }
                flex
            }
            LayoutMode::Grid => vec![
                ("display", "grid".to_owned()),
                (
                    "grid-template-columns",
                    format!("repeat({}, minmax(0, 1fr))", self.columns.max(1)),
                ),
            ],
        };
        out.push(("gap", self.gap.to_css()));
        out.push(("align-items", self.align.as_css().to_owned()));
        if self.mode == LayoutMode::Flex {
            out.push(("justify-content", self.justify.as_css().to_owned()));
        }
        out
    }
}

macro_rules! css_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $json:literal, $css:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $json)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The name used in JSON and in the editor's controls.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $json),+ }
            }

            pub fn as_css(self) -> &'static str {
                match self { $($name::$variant => $css),+ }
            }

            pub fn parse(s: &str) -> Option<Self> {
                match s { $($json => Some($name::$variant),)+ _ => None }
            }
        }
    };
}

css_enum!(LayoutMode { Flex => "flex", "flex", Grid => "grid", "grid" });
css_enum!(Direction { Row => "row", "row", Column => "column", "column" });
css_enum!(AlignItems {
    Start => "start", "flex-start",
    Center => "center", "center",
    End => "end", "flex-end",
    Stretch => "stretch", "stretch",
});
css_enum!(Justify {
    Start => "start", "flex-start",
    Center => "center", "center",
    End => "end", "flex-end",
    Between => "between", "space-between",
});

/// A viewport width below which a node's [`Override`]s apply. Styles are desktop-first: the
/// base style is the widest layout, and each breakpoint adjusts it for narrower screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Breakpoint {
    Tablet,
    Mobile,
}

impl Breakpoint {
    /// Widest first, the order their rules cascade in.
    pub const ALL: [Breakpoint; 2] = [Breakpoint::Tablet, Breakpoint::Mobile];

    pub fn as_str(self) -> &'static str {
        match self {
            Breakpoint::Tablet => "tablet",
            Breakpoint::Mobile => "mobile",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Breakpoint::Tablet => "Tablet",
            Breakpoint::Mobile => "Mobile",
        }
    }

    /// The overrides apply at this width in px and below.
    pub fn max_width(self) -> u32 {
        match self {
            Breakpoint::Tablet => 900,
            Breakpoint::Mobile => 600,
        }
    }

    pub fn media(self) -> String {
        format!("@media (max-width: {}px)", self.max_width())
    }
}

/// Changes to a node's style at one [`Breakpoint`]. Unset fields keep the wider value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Override {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<Length>,
    /// Name of a type token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Containers with a flex layout only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    /// Containers with a grid layout only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<u8>,
    /// Containers only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap: Option<Length>,
}

impl Override {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Layers `narrower` on top of this one: its set fields win.
    pub fn merge(&mut self, narrower: &Override) {
        self.hidden |= narrower.hidden;
        macro_rules! take {
            ($($field:ident),+) => {
                $(if narrower.$field.is_some() {
                    self.$field = narrower.$field.clone();
                })+
            };
        }
        take!(align, padding, font, direction, columns, gap);
    }

    pub fn apply_style(&self, style: &Style) -> Style {
        Style {
            align: self.align.unwrap_or(style.align),
            padding: self
                .padding
                .clone()
                .unwrap_or_else(|| style.padding.clone()),
            font: self.font.clone().or_else(|| style.font.clone()),
            ..style.clone()
        }
    }

    pub fn apply_layout(&self, layout: &Layout) -> Layout {
        Layout {
            direction: self.direction.unwrap_or(layout.direction),
            columns: self.columns.unwrap_or(layout.columns),
            gap: self.gap.clone().unwrap_or_else(|| layout.gap.clone()),
            ..layout.clone()
        }
    }

    /// The CSS declarations this override adds inside its breakpoint's media query, for `node`.
    pub fn declarations(&self, node: &Node) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if self.hidden {
            out.push(("display", "none".to_owned()));
        }
        // A spacer's look is its height and background, so only hiding applies to it.
        if matches!(node.kind, NodeKind::Spacer { .. }) {
            return out;
        }
        if let Some(align) = self.align {
            out.push(("text-align", align.as_css().to_owned()));
        }
        if let Some(padding) = &self.padding {
            out.push(("padding", padding.to_css()));
        }
        if let Some(font) = &self.font {
            out.push(("font", css::var("font", font)));
        }
        if let NodeKind::Container { layout, .. } = &node.kind {
            match (layout.mode, self.direction, self.columns) {
                (LayoutMode::Flex, Some(direction), _) => {
                    out.push(("flex-direction", direction.as_css().to_owned()))
                }
                (LayoutMode::Grid, _, Some(columns)) => out.push((
                    "grid-template-columns",
                    format!("repeat({}, minmax(0, 1fr))", columns.max(1)),
                )),
                _ => {}
            }
            if let Some(gap) = &self.gap {
                out.push(("gap", gap.to_css()));
            }
        }
        out
    }
}

impl Node {
    /// Every override that applies at `breakpoint` (`None` is desktop), merged widest first.
    pub fn override_at(&self, breakpoint: Option<Breakpoint>) -> Override {
        let mut out = Override::default();
        let Some(narrowest) = breakpoint else {
            return out;
        };
        for bp in Breakpoint::ALL.into_iter().filter(|bp| *bp <= narrowest) {
            if let Some(o) = self.responsive.get(&bp) {
                out.merge(o);
            }
        }
        out
    }
}

css_enum!(Easing {
    Linear => "linear", "linear",
    Ease => "ease", "ease",
    EaseIn => "ease-in", "ease-in",
    EaseOut => "ease-out", "ease-out",
    EaseInOut => "ease-in-out", "ease-in-out",
});

// `css_enum!` writes the enum, so the default can't be a derive attribute on it.
#[allow(clippy::derivable_impls)]
impl Default for Easing {
    fn default() -> Self {
        Easing::EaseOut
    }
}

/// A keyframe animation of opacity and transform. Exports turn it into CSS `@keyframes`, so it
/// plays in every target and in exported SVG files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    /// In seconds.
    pub duration: f64,
    /// In seconds.
    #[serde(default)]
    pub delay: f64,
    #[serde(default)]
    pub easing: Easing,
    /// How many times it plays; `0` loops forever.
    #[serde(default = "one_u32")]
    pub iterations: u32,
    /// Play every other iteration backwards.
    #[serde(default)]
    pub alternate: bool,
    /// Sorted by `at`.
    pub keyframes: Vec<Keyframe>,
}

/// The state at one point of an [`Animation`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    /// Percent of the way through, 0 to 100.
    pub at: f64,
    #[serde(default = "one_f64")]
    pub opacity: f64,
    /// Offset in px.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default = "one_f64")]
    pub scale: f64,
    /// In degrees.
    #[serde(default)]
    pub rotate: f64,
}

fn one_u32() -> u32 {
    1
}

fn one_f64() -> f64 {
    1.0
}

impl Keyframe {
    /// No change from the element's own look, at `at` percent.
    pub fn rest(at: f64) -> Self {
        Self {
            at,
            opacity: 1.0,
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            rotate: 0.0,
        }
    }

    pub fn declarations(&self) -> Vec<(&'static str, String)> {
        vec![
            ("opacity", css::number(self.opacity.clamp(0.0, 1.0))),
            (
                "transform",
                format!(
                    "translate({}px, {}px) rotate({}deg) scale({})",
                    css::number(self.x),
                    css::number(self.y),
                    css::number(self.rotate),
                    css::number(self.scale)
                ),
            ),
        ]
    }
}

impl Animation {
    /// Ready-made animations the editor offers, as (id, label).
    pub const PRESETS: &'static [(&'static str, &'static str)] = &[
        ("fade-in", "Fade in"),
        ("slide-up", "Slide up"),
        ("pop", "Pop in"),
        ("spin", "Spin"),
        ("pulse", "Pulse"),
        ("float", "Float"),
    ];

    pub fn preset(id: &str) -> Option<Self> {
        let k = Keyframe::rest;
        let once = |duration, keyframes| Self {
            duration,
            delay: 0.0,
            easing: Easing::EaseOut,
            iterations: 1,
            alternate: false,
            keyframes,
        };
        Some(match id {
            "fade-in" => once(
                0.6,
                vec![
                    Keyframe {
                        opacity: 0.0,
                        ..k(0.0)
                    },
                    k(100.0),
                ],
            ),
            "slide-up" => once(
                0.6,
                vec![
                    Keyframe {
                        opacity: 0.0,
                        y: 24.0,
                        ..k(0.0)
                    },
                    k(100.0),
                ],
            ),
            "pop" => once(
                0.5,
                vec![
                    Keyframe {
                        opacity: 0.0,
                        scale: 0.6,
                        ..k(0.0)
                    },
                    Keyframe {
                        scale: 1.08,
                        ..k(70.0)
                    },
                    k(100.0),
                ],
            ),
            "spin" => Self {
                easing: Easing::Linear,
                iterations: 0,
                ..once(
                    4.0,
                    vec![
                        k(0.0),
                        Keyframe {
                            rotate: 360.0,
                            ..k(100.0)
                        },
                    ],
                )
            },
            "pulse" => Self {
                easing: Easing::EaseInOut,
                iterations: 0,
                alternate: true,
                ..once(
                    1.2,
                    vec![
                        k(0.0),
                        Keyframe {
                            scale: 1.08,
                            ..k(100.0)
                        },
                    ],
                )
            },
            "float" => Self {
                easing: Easing::EaseInOut,
                iterations: 0,
                alternate: true,
                ..once(
                    2.0,
                    vec![
                        k(0.0),
                        Keyframe {
                            y: -12.0,
                            ..k(100.0)
                        },
                    ],
                )
            },
            _ => return None,
        })
    }

    /// The `@keyframes` body: one (stop, declarations) pair per keyframe, such as `"50%"`.
    pub fn keyframe_rules(&self) -> Vec<(String, Vec<(&'static str, String)>)> {
        let mut keyframes = self.keyframes.clone();
        keyframes.sort_by(|a, b| a.at.total_cmp(&b.at));
        keyframes
            .iter()
            .map(|k| {
                (
                    format!("{}%", css::number(k.at.clamp(0.0, 100.0))),
                    k.declarations(),
                )
            })
            .collect()
    }

    /// The declarations that play `@keyframes name` on an element. Shapes in an SVG pivot on
    /// their own center rather than the drawing's origin.
    pub fn declarations(&self, name: &str, svg_shape: bool) -> Vec<(&'static str, String)> {
        let iterations = match self.iterations {
            0 => "infinite".to_owned(),
            n => n.to_string(),
        };
        let direction = if self.alternate {
            "alternate"
        } else {
            "normal"
        };
        let mut out = vec![(
            "animation",
            format!(
                "{name} {}s {} {}s {iterations} {direction} both",
                css::number(self.duration.max(0.01)),
                self.easing.as_css(),
                css::number(self.delay.max(0.0)),
            ),
        )];
        if svg_shape {
            out.push(("transform-box", "fill-box".to_owned()));
            out.push(("transform-origin", "center".to_owned()));
        }
        out
    }
}

/// CSS spelling shared by the editor canvas and the exporters.
pub mod css {
    use super::parse_token_ref;

    /// Lowercase letters, digits and single dashes, so a token name is a valid CSS identifier.
    pub fn slug(name: &str) -> String {
        let mut out = String::new();
        for c in name.trim().to_lowercase().chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else if !out.is_empty() && !out.ends_with('-') {
                out.push('-');
            }
        }
        out.trim_end_matches('-').to_owned()
    }

    /// The custom property a token is stored in, e.g. `--color-accent`.
    pub fn property(prefix: &str, name: &str) -> String {
        format!("--{prefix}-{}", slug(name))
    }

    pub fn var(prefix: &str, name: &str) -> String {
        format!("var({})", property(prefix, name))
    }

    /// A color value: token references become `var(--color-*)`, literals pass through.
    pub fn color(value: &str) -> String {
        match parse_token_ref(value) {
            Some(name) => var("color", name),
            None => value.to_owned(),
        }
    }

    /// Rounds to two decimals and drops a trailing `.0`.
    pub fn number(n: f64) -> String {
        let r = (n * 100.0).round() / 100.0;
        if r == 0.0 {
            "0".into()
        } else {
            format!("{r}")
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A vector drawing: a fixed-size SVG viewport holding shapes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub width: f64,
    pub height: f64,
    pub shapes: Vec<Shape>,
}

impl Default for Drawing {
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 600.0,
            shapes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub id: NodeId,
    pub style: ShapeStyle,
    #[serde(flatten)]
    pub geometry: Geometry,
    /// Played when the drawing is shown. Transforms pivot on the shape's own center.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<Animation>,
}

impl Shape {
    pub fn new(id: impl Into<NodeId>, style: ShapeStyle, geometry: Geometry) -> Self {
        Self {
            id: id.into(),
            style,
            geometry,
            animation: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeStyle {
    pub fill: String,
    pub stroke: String,
    pub stroke_width: f64,
}

impl Default for ShapeStyle {
    fn default() -> Self {
        Self {
            fill: "#7c5cff".into(),
            stroke: "#1f1b2e".into(),
            stroke_width: 2.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Geometry {
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    Ellipse {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    Path {
        nodes: Vec<PathNode>,
        closed: bool,
        /// Further closed outlines of the same shape, such as the hole in a ring. Filled with
        /// the even-odd rule.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        subpaths: Vec<Vec<PathNode>>,
    },
}

impl Geometry {
    /// Whether the shape has an interior that `fill` applies to.
    pub fn is_filled(&self) -> bool {
        match self {
            Geometry::Rect { .. } | Geometry::Ellipse { .. } => true,
            Geometry::Line { .. } => false,
            Geometry::Path {
                closed, subpaths, ..
            } => *closed || !subpaths.is_empty(),
        }
    }
}

/// An anchor on a pen path. `handle_in`/`handle_out` are absolute bezier control points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathNode {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle_in: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle_out: Option<Point>,
}

impl PathNode {
    pub fn corner(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            handle_in: None,
            handle_out: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_round_trips_through_json() {
        let mut doc = Document {
            title: "Demo".into(),
            ..Document::default()
        };
        doc.pages[0].nodes = vec![
            Node {
                id: "h".into(),
                style: Style::default(),
                kind: NodeKind::Heading {
                    text: "Hi".into(),
                    level: 1,
                },
                responsive: BTreeMap::new(),
                bindings: BTreeMap::new(),
                animation: None,
            },
            Node {
                id: "v".into(),
                style: Style {
                    align: Align::Center,
                    ..Style::default()
                },
                kind: NodeKind::Svg {
                    drawing: Drawing {
                        width: 100.0,
                        height: 50.0,
                        shapes: vec![Shape {
                            id: "p".into(),
                            style: ShapeStyle::default(),
                            geometry: Geometry::Path {
                                nodes: vec![
                                    PathNode::corner(0.0, 0.0),
                                    PathNode {
                                        handle_out: Some(Point::new(5.0, 5.0)),
                                        ..PathNode::corner(10.0, 0.0)
                                    },
                                ],
                                closed: false,
                                subpaths: vec![],
                            },
                            animation: Animation::preset("spin"),
                        }],
                    },
                },
                responsive: BTreeMap::new(),
                bindings: BTreeMap::new(),
                animation: None,
            },
            Node {
                id: "row".into(),
                style: Style::default(),
                kind: NodeKind::Container {
                    layout: Layout::row(),
                    children: vec![Node {
                        id: "c".into(),
                        style: Style {
                            padding: Length::Px(4.0),
                            font: Some("body".into()),
                            ..Style::default()
                        },
                        kind: NodeKind::Text { text: "in".into() },
                        responsive: BTreeMap::new(),
                        bindings: BTreeMap::new(),
                        animation: None,
                    }],
                },
                responsive: BTreeMap::from([(
                    Breakpoint::Mobile,
                    Override {
                        direction: Some(Direction::Column),
                        hidden: true,
                        ..Override::default()
                    },
                )]),
                bindings: BTreeMap::new(),
                animation: Animation::preset("pop"),
            },
        ];
        doc.pages.push(Page {
            id: "about".into(),
            name: "About".into(),
            path: "/about".into(),
            nodes: vec![
                Node::new("n", NodeKind::Nav {}),
                Node::new(
                    "i",
                    NodeKind::Instance {
                        component: "card".into(),
                        props: BTreeMap::from([("title".into(), "Hi".into())]),
                    },
                ),
            ],
        });
        let mut title = Node::new(
            "ct",
            NodeKind::Heading {
                text: "Card".into(),
                level: 2,
            },
        );
        title.bindings.insert("text".into(), "title".into());
        doc.components.push(Component {
            id: "card".into(),
            name: "Card".into(),
            props: vec![PropDef {
                name: "title".into(),
                kind: PropKind::Text,
                default: "Card".into(),
            }],
            nodes: vec![title],
        });
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), doc);
    }

    #[test]
    fn node_json_is_flat_and_tagged() {
        let node = Node::new("t", NodeKind::Text { text: "x".into() });
        let v = serde_json::to_value(&node).unwrap();
        assert_eq!(v["type"], "text");
        assert_eq!(v["text"], "x");
        assert_eq!(v["style"]["align"], "left");
        assert_eq!(v["style"]["color"], "{text}");
        assert_eq!(v["style"]["padding"], "{md}");
        assert!(v.get("responsive").is_none());
    }

    #[test]
    fn version_2_documents_become_a_home_page() {
        let json = r#"{"version":2,"title":"Two","tokens":{"colors":[],"fonts":[],"spacing":[]},
            "nodes":[{"id":"t","type":"text","text":"x","style":{"align":"left","color":"{text}",
            "background":null,"padding":"{md}"}}]}"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.pages[0].path, "/");
        assert_eq!(doc.pages[0].nodes[0].id, "t");
        assert!(serde_json::to_value(&doc).unwrap().get("nodes").is_none());
    }

    #[test]
    fn finds_pages_by_link() {
        let mut doc = Document::default();
        doc.pages.push(Page {
            id: "a".into(),
            name: "About".into(),
            path: "/about".into(),
            nodes: vec![],
        });
        let id = |href| doc.page_for_link(href).map(|p| p.id.as_str());
        assert_eq!(id("/about"), Some("a"));
        assert_eq!(id("/about/#team"), Some("a"));
        assert_eq!(id("/?q=1"), Some("home"));
        assert_eq!(id("about"), None);
        assert_eq!(id("https://example.com/about"), None);
        assert_eq!(doc.pages[1].file_stem(), "about");
        assert_eq!(doc.pages[0].file_stem(), "index");
    }

    #[test]
    fn overrides_cascade_from_wider_breakpoints() {
        let mut node = Node::new(
            "g",
            NodeKind::Container {
                layout: Layout::grid(3),
                children: vec![],
            },
        );
        node.responsive.insert(
            Breakpoint::Tablet,
            Override {
                columns: Some(2),
                align: Some(Align::Center),
                ..Override::default()
            },
        );
        node.responsive.insert(
            Breakpoint::Mobile,
            Override {
                columns: Some(1),
                direction: Some(Direction::Row),
                ..Override::default()
            },
        );
        assert!(node.override_at(None).is_empty());
        let mobile = node.override_at(Some(Breakpoint::Mobile));
        assert_eq!(mobile.columns, Some(1));
        assert_eq!(mobile.align, Some(Align::Center));
        assert_eq!(mobile.apply_style(&node.style).align, Align::Center);
        // Direction is flex-only, so a grid ignores it.
        assert_eq!(
            node.responsive[&Breakpoint::Mobile].declarations(&node),
            [(
                "grid-template-columns",
                "repeat(1, minmax(0, 1fr))".to_owned()
            )]
        );
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["responsive"]["tablet"]["columns"], 2);
        assert!(json["responsive"]["tablet"].get("hidden").is_none());
    }

    #[test]
    fn animations_lower_to_keyframes_and_a_shorthand() {
        let mut a = Animation::preset("pop").unwrap();
        a.keyframes.reverse();
        let rules = a.keyframe_rules();
        assert_eq!(rules[0].0, "0%");
        assert_eq!(
            rules[0].1,
            [
                ("opacity", "0".to_owned()),
                (
                    "transform",
                    "translate(0px, 0px) rotate(0deg) scale(0.6)".to_owned()
                )
            ]
        );
        assert_eq!(rules[1].0, "70%");
        assert_eq!(
            Animation::preset("spin")
                .unwrap()
                .declarations("spin-1", true),
            [
                (
                    "animation",
                    "spin-1 4s linear 0s infinite normal both".to_owned()
                ),
                ("transform-box", "fill-box".to_owned()),
                ("transform-origin", "center".to_owned())
            ]
        );
        for (id, _) in Animation::PRESETS {
            assert!(Animation::preset(id).is_some(), "{id}");
        }
        let json: Keyframe = serde_json::from_str(r#"{"at":50}"#).unwrap();
        assert_eq!(json, Keyframe::rest(50.0));
    }

    #[test]
    fn hiding_a_spacer_is_its_only_override() {
        let spacer = Node::new("s", NodeKind::Spacer { height: 8.0 });
        let o = Override {
            hidden: true,
            padding: Some(Length::Px(4.0)),
            ..Override::default()
        };
        assert_eq!(o.declarations(&spacer), [("display", "none".to_owned())]);
    }

    #[test]
    fn version_1_documents_still_load() {
        let json = r##"{"version":1,"title":"Old","nodes":[{"id":"t","type":"text","text":"x",
            "style":{"align":"left","color":"#000","background":null,"padding":16}}]}"##;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.version, SCHEMA_VERSION);
        assert_eq!(doc.tokens, Tokens::default());
        let home = &doc.pages[0];
        assert!(home.is_home());
        assert_eq!(home.nodes[0].style.padding, Length::Px(16.0));
        assert_eq!(home.nodes[0].style.font, None);
        assert!(home.nodes[0].responsive.is_empty());
    }

    #[test]
    fn rejects_lengths_that_are_not_token_references() {
        assert!(serde_json::from_str::<Length>(r#""16px""#).is_err());
        assert_eq!(
            serde_json::from_str::<Length>(r#""{lg}""#).unwrap(),
            Length::Token("lg".into())
        );
    }

    #[test]
    fn styles_resolve_tokens_to_custom_properties() {
        let style = Style {
            background: Some("#fafafa".into()),
            font: Some("Body Copy".into()),
            ..Style::default()
        };
        assert_eq!(
            style.declarations(),
            [
                ("text-align", "left".to_owned()),
                ("color", "var(--color-text)".to_owned()),
                ("background", "#fafafa".to_owned()),
                ("padding", "var(--space-md)".to_owned()),
                ("font", "var(--font-body-copy)".to_owned()),
            ]
        );
    }

    #[test]
    fn tokens_emit_light_values_and_dark_overrides() {
        let tokens = Tokens::default();
        let light = tokens.variables(false);
        assert!(light.contains(&("--color-text".into(), "#1f1b2e".into())));
        assert!(light.contains(&("--space-md".into(), "16px".into())));
        assert!(light.contains(&("--font-body".into(), format!("400 16px/1.6 {SYSTEM_FONT}"))));
        let dark = tokens.dark_overrides();
        assert_eq!(dark.len(), tokens.colors.len());
        assert!(dark.contains(&("--color-text".into(), "#f4f3f8".into())));
    }

    #[test]
    fn layouts_lower_to_flex_and_grid() {
        assert_eq!(
            Layout::row().declarations(),
            [
                ("display", "flex".to_owned()),
                ("flex-direction", "row".to_owned()),
                ("flex-wrap", "wrap".to_owned()),
                ("gap", "var(--space-md)".to_owned()),
                ("align-items", "center".to_owned()),
                ("justify-content", "flex-start".to_owned()),
            ]
        );
        assert_eq!(
            Layout::grid(3).declarations()[1],
            (
                "grid-template-columns",
                "repeat(3, minmax(0, 1fr))".to_owned()
            )
        );
    }
}
