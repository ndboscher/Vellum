//! The Vellum page document.
//!
//! Everything else in the workspace reads and writes these types: the editor
//! mutates them through `vellum-core`, the vector tools build [`Shape`]s, the
//! exporters turn a [`Document`] into code and the server stores it as JSON.

use serde::{Deserialize, Serialize};

/// Bumped whenever the serialized shape changes, so old files can be migrated.
///
/// Version 2 added design [`Tokens`] and [`NodeKind::Container`]. Version 1 files still load:
/// they get the default tokens, and their flat block list becomes the page's top level.
pub const SCHEMA_VERSION: u32 = 2;

pub type NodeId = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
    pub title: String,
    #[serde(default)]
    pub tokens: Tokens,
    /// The page's top level, stacked vertically. Containers hold further nodes.
    pub nodes: Vec<Node>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            title: "Untitled site".into(),
            tokens: Tokens::default(),
            nodes: Vec::new(),
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
}

impl Node {
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
        }
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
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64 },
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    Path { nodes: Vec<PathNode>, closed: bool },
}

impl Geometry {
    /// Whether the shape has an interior that `fill` applies to.
    pub fn is_filled(&self) -> bool {
        match self {
            Geometry::Rect { .. } | Geometry::Ellipse { .. } => true,
            Geometry::Line { .. } => false,
            Geometry::Path { closed, .. } => *closed,
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
        let doc = Document {
            title: "Demo".into(),
            nodes: vec![
                Node {
                    id: "h".into(),
                    style: Style::default(),
                    kind: NodeKind::Heading {
                        text: "Hi".into(),
                        level: 1,
                    },
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
                                },
                            }],
                        },
                    },
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
                        }],
                    },
                },
            ],
            ..Document::default()
        };
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), doc);
    }

    #[test]
    fn node_json_is_flat_and_tagged() {
        let node = Node {
            id: "t".into(),
            style: Style::default(),
            kind: NodeKind::Text { text: "x".into() },
        };
        let v = serde_json::to_value(&node).unwrap();
        assert_eq!(v["type"], "text");
        assert_eq!(v["text"], "x");
        assert_eq!(v["style"]["align"], "left");
        assert_eq!(v["style"]["color"], "{text}");
        assert_eq!(v["style"]["padding"], "{md}");
    }

    #[test]
    fn version_1_documents_still_load() {
        let json = r##"{"version":1,"title":"Old","nodes":[{"id":"t","type":"text","text":"x",
            "style":{"align":"left","color":"#000","background":null,"padding":16}}]}"##;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.tokens, Tokens::default());
        assert_eq!(doc.nodes[0].style.padding, Length::Px(16.0));
        assert_eq!(doc.nodes[0].style.font, None);
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
