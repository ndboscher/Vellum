//! The Vellum page document.
//!
//! Everything else in the workspace reads and writes these types: the editor
//! mutates them through `vellum-core`, the vector tools build [`Shape`]s, the
//! exporters turn a [`Document`] into code and the server stores it as JSON.

use serde::{Deserialize, Serialize};

/// Bumped whenever the serialized shape changes, so old files can be migrated.
pub const SCHEMA_VERSION: u32 = 1;

pub type NodeId = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
    pub title: String,
    pub nodes: Vec<Node>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            title: "Untitled site".into(),
            nodes: Vec::new(),
        }
    }
}

/// One block on the page.
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
    Heading { text: String, level: u8 },
    Text { text: String },
    Button { label: String, href: String },
    Image { src: String, alt: String },
    Spacer { height: f64 },
    Svg { drawing: Drawing },
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
    /// CSS color.
    pub color: String,
    /// CSS color, or `None` for transparent.
    pub background: Option<String>,
    /// Padding on all sides, in px.
    pub padding: f64,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            align: Align::Left,
            color: "#1f1b2e".into(),
            background: None,
            padding: 16.0,
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
            version: SCHEMA_VERSION,
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
            ],
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
    }
}
