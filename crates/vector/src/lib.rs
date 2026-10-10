//! Geometry helpers for the vector tools and SVG output.

use std::fmt::Write;
use vellum_schema::{Drawing, Geometry, PathNode, Point, Shape};

/// Rounds to two decimals so generated SVG stays short and stable.
fn fmt(n: f64) -> String {
    let r = (n * 100.0).round() / 100.0;
    if r == 0.0 {
        "0".into()
    } else {
        format!("{r}")
    }
}

/// Normalised box `(x, y, w, h)` from two drag corners.
pub fn box_from_drag(a: Point, b: Point) -> (f64, f64, f64, f64) {
    (
        a.x.min(b.x),
        a.y.min(b.y),
        (b.x - a.x).abs(),
        (b.y - a.y).abs(),
    )
}

/// Geometry produced by dragging from `a` to `b` with a shape tool.
pub fn rect_from_drag(a: Point, b: Point) -> Geometry {
    let (x, y, w, h) = box_from_drag(a, b);
    Geometry::Rect { x, y, w, h }
}

pub fn ellipse_from_drag(a: Point, b: Point) -> Geometry {
    let (x, y, w, h) = box_from_drag(a, b);
    Geometry::Ellipse {
        cx: x + w / 2.0,
        cy: y + h / 2.0,
        rx: w / 2.0,
        ry: h / 2.0,
    }
}

pub fn line_from_drag(a: Point, b: Point) -> Geometry {
    Geometry::Line {
        x1: a.x,
        y1: a.y,
        x2: b.x,
        y2: b.y,
    }
}

/// Whether a freshly drawn shape is big enough to keep (filters out stray clicks).
pub fn is_meaningful(g: &Geometry) -> bool {
    match g {
        Geometry::Rect { w, h, .. } => *w > 2.0 && *h > 2.0,
        Geometry::Ellipse { rx, ry, .. } => *rx > 1.0 && *ry > 1.0,
        Geometry::Line { x1, y1, x2, y2 } => (x2 - x1).hypot(y2 - y1) > 2.0,
        Geometry::Path { nodes, .. } => nodes.len() >= 2,
    }
}

/// SVG path data for a pen path: straight segments unless either end carries a handle.
pub fn path_data(nodes: &[PathNode], closed: bool) -> String {
    let Some(first) = nodes.first() else {
        return String::new();
    };
    let mut d = format!("M{} {}", fmt(first.x), fmt(first.y));
    let segment = |d: &mut String, from: &PathNode, to: &PathNode| {
        if from.handle_out.is_none() && to.handle_in.is_none() {
            let _ = write!(d, " L{} {}", fmt(to.x), fmt(to.y));
        } else {
            let c1 = from.handle_out.unwrap_or(Point::new(from.x, from.y));
            let c2 = to.handle_in.unwrap_or(Point::new(to.x, to.y));
            let _ = write!(
                d,
                " C{} {} {} {} {} {}",
                fmt(c1.x),
                fmt(c1.y),
                fmt(c2.x),
                fmt(c2.y),
                fmt(to.x),
                fmt(to.y)
            );
        }
    };
    for pair in nodes.windows(2) {
        segment(&mut d, &pair[0], &pair[1]);
    }
    if closed && nodes.len() > 2 {
        segment(&mut d, &nodes[nodes.len() - 1], first);
        d.push_str(" Z");
    }
    d
}

/// Sets symmetric handles on an anchor from a drag to `pull` (the outgoing handle).
pub fn pull_handles(node: PathNode, pull: Point) -> PathNode {
    if (pull.x - node.x).hypot(pull.y - node.y) < 3.0 {
        return PathNode::corner(node.x, node.y);
    }
    PathNode {
        handle_out: Some(pull),
        handle_in: Some(Point::new(2.0 * node.x - pull.x, 2.0 * node.y - pull.y)),
        ..node
    }
}

fn shift(p: Point, dx: f64, dy: f64) -> Point {
    Point::new(p.x + dx, p.y + dy)
}

pub fn translate(g: &Geometry, dx: f64, dy: f64) -> Geometry {
    match g {
        Geometry::Rect { x, y, w, h } => Geometry::Rect {
            x: x + dx,
            y: y + dy,
            w: *w,
            h: *h,
        },
        Geometry::Ellipse { cx, cy, rx, ry } => Geometry::Ellipse {
            cx: cx + dx,
            cy: cy + dy,
            rx: *rx,
            ry: *ry,
        },
        Geometry::Line { x1, y1, x2, y2 } => Geometry::Line {
            x1: x1 + dx,
            y1: y1 + dy,
            x2: x2 + dx,
            y2: y2 + dy,
        },
        Geometry::Path { nodes, closed } => Geometry::Path {
            nodes: nodes
                .iter()
                .map(|n| PathNode {
                    x: n.x + dx,
                    y: n.y + dy,
                    handle_in: n.handle_in.map(|p| shift(p, dx, dy)),
                    handle_out: n.handle_out.map(|p| shift(p, dx, dy)),
                })
                .collect(),
            closed: *closed,
        },
    }
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// One shape as an SVG element string.
pub fn shape_to_svg(shape: &Shape) -> String {
    let fill = if shape.geometry.is_filled() {
        escape_attr(&shape.style.fill)
    } else {
        "none".into()
    };
    let style = format!(
        r#"fill="{fill}" stroke="{}" stroke-width="{}""#,
        escape_attr(&shape.style.stroke),
        fmt(shape.style.stroke_width)
    );
    match &shape.geometry {
        Geometry::Rect { x, y, w, h } => {
            format!(
                r#"<rect x="{}" y="{}" width="{}" height="{}" {style}/>"#,
                fmt(*x),
                fmt(*y),
                fmt(*w),
                fmt(*h)
            )
        }
        Geometry::Ellipse { cx, cy, rx, ry } => format!(
            r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" {style}/>"#,
            fmt(*cx),
            fmt(*cy),
            fmt(*rx),
            fmt(*ry)
        ),
        Geometry::Line { x1, y1, x2, y2 } => format!(
            r#"<line x1="{}" y1="{}" x2="{}" y2="{}" {style} stroke-linecap="round"/>"#,
            fmt(*x1),
            fmt(*y1),
            fmt(*x2),
            fmt(*y2)
        ),
        Geometry::Path { nodes, closed } => format!(
            r#"<path d="{}" {style} stroke-linejoin="round" stroke-linecap="round"/>"#,
            path_data(nodes, *closed)
        ),
    }
}

/// A complete standalone SVG file for a drawing.
pub fn drawing_to_svg(drawing: &Drawing) -> String {
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">"#,
        w = fmt(drawing.width),
        h = fmt(drawing.height)
    );
    out.push('\n');
    for s in &drawing.shapes {
        let _ = writeln!(out, "  {}", shape_to_svg(s));
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vellum_schema::ShapeStyle;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn path_data_uses_lines_between_corners() {
        assert_eq!(
            path_data(
                &[PathNode::corner(0.0, 0.0), PathNode::corner(10.0, 5.0)],
                false
            ),
            "M0 0 L10 5"
        );
    }

    #[test]
    fn path_data_uses_curves_and_closes() {
        let nodes = [
            PathNode {
                handle_out: Some(p(5.0, -5.0)),
                ..PathNode::corner(0.0, 0.0)
            },
            PathNode::corner(10.0, 0.0),
            PathNode::corner(10.0, 10.0),
        ];
        assert_eq!(
            path_data(&nodes, true),
            "M0 0 C5 -5 10 0 10 0 L10 10 L0 0 Z"
        );
    }

    #[test]
    fn two_point_path_never_closes() {
        assert_eq!(
            path_data(
                &[PathNode::corner(0.0, 0.0), PathNode::corner(1.0, 1.0)],
                true
            ),
            "M0 0 L1 1"
        );
    }

    #[test]
    fn drag_boxes_normalise_direction() {
        assert_eq!(
            box_from_drag(p(10.0, 20.0), p(4.0, 2.0)),
            (4.0, 2.0, 6.0, 18.0)
        );
        assert_eq!(
            ellipse_from_drag(p(10.0, 10.0), p(0.0, 0.0)),
            Geometry::Ellipse {
                cx: 5.0,
                cy: 5.0,
                rx: 5.0,
                ry: 5.0
            }
        );
    }

    #[test]
    fn small_pulls_leave_a_corner() {
        assert_eq!(
            pull_handles(PathNode::corner(0.0, 0.0), p(1.0, 1.0)),
            PathNode::corner(0.0, 0.0)
        );
        let n = pull_handles(PathNode::corner(10.0, 10.0), p(20.0, 10.0));
        assert_eq!(n.handle_out, Some(p(20.0, 10.0)));
        assert_eq!(n.handle_in, Some(p(0.0, 10.0)));
    }

    #[test]
    fn translate_moves_anchors_and_handles() {
        let g = Geometry::Path {
            nodes: vec![PathNode {
                handle_out: Some(p(2.0, 2.0)),
                ..PathNode::corner(1.0, 1.0)
            }],
            closed: false,
        };
        let Geometry::Path { nodes, .. } = translate(&g, 3.0, 4.0) else {
            panic!()
        };
        assert_eq!(
            nodes[0],
            PathNode {
                handle_out: Some(p(5.0, 6.0)),
                ..PathNode::corner(4.0, 5.0)
            }
        );
    }

    #[test]
    fn svg_output_leaves_open_paths_unfilled() {
        let style = ShapeStyle::default();
        let svg = drawing_to_svg(&Drawing {
            width: 100.0,
            height: 50.0,
            shapes: vec![
                Shape {
                    id: "r".into(),
                    style: style.clone(),
                    geometry: Geometry::Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                    },
                },
                Shape {
                    id: "p".into(),
                    style,
                    geometry: Geometry::Path {
                        nodes: vec![PathNode::corner(0.0, 0.0), PathNode::corner(5.0, 5.0)],
                        closed: false,
                    },
                },
            ],
        });
        assert!(svg.contains(r#"viewBox="0 0 100 50""#));
        assert!(svg.contains(r##"<rect x="0" y="0" width="10" height="10" fill="#7c5cff""##));
        assert!(svg.contains(r#"<path d="M0 0 L5 5" fill="none""#));
    }
}
