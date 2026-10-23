//! Reading SVG files into shapes. `usvg` resolves transforms, styles and shape elements, so
//! every visible outline arrives as absolute path segments.

use usvg::tiny_skia_path::{PathSegment, Point as SkPoint};
use usvg::{Group, Node, Paint};
use vellum_schema::{Drawing, Geometry, PathNode, Point, Shape, ShapeStyle};

/// Parses an SVG document into a drawing of its size. Closed outlines of one element become one
/// path (extra outlines as subpaths); open ones become their own paths. Gradients and patterns
/// fall back to a neutral gray, and text and images are skipped.
pub fn import_svg(text: &str, mut next_id: impl FnMut() -> String) -> Result<Drawing, String> {
    let tree = usvg::Tree::from_str(text, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let mut shapes = Vec::new();
    collect(tree.root(), &mut shapes, &mut next_id);
    if shapes.is_empty() {
        return Err("no shapes found in this SVG".into());
    }
    let size = tree.size();
    Ok(Drawing {
        width: round(size.width() as f64),
        height: round(size.height() as f64),
        shapes,
    })
}

fn collect(group: &Group, out: &mut Vec<Shape>, next_id: &mut impl FnMut() -> String) {
    for node in group.children() {
        match node {
            Node::Group(g) => collect(g, out, next_id),
            Node::Path(path) if path.is_visible() => {
                let Some(data) = path.data().clone().transform(path.abs_transform()) else {
                    continue;
                };
                let style = ShapeStyle {
                    fill: path.fill().map_or("none".into(), |f| color(f.paint())),
                    stroke: path.stroke().map_or("none".into(), |s| color(s.paint())),
                    stroke_width: path.stroke().map_or(0.0, |s| round(s.width().get() as f64)),
                };
                let (closed, open) = outlines(data.segments());
                let mut closed = closed.into_iter();
                if let Some(nodes) = closed.next() {
                    let geometry = Geometry::Path {
                        nodes,
                        closed: true,
                        subpaths: closed.collect(),
                    };
                    out.push(Shape::new(next_id(), style.clone(), geometry));
                }
                for nodes in open {
                    let geometry = Geometry::Path {
                        nodes,
                        closed: false,
                        subpaths: Vec::new(),
                    };
                    out.push(Shape::new(next_id(), style.clone(), geometry));
                }
            }
            _ => {}
        }
    }
}

/// Splits path segments into closed and open pen outlines.
fn outlines(
    segments: impl Iterator<Item = PathSegment>,
) -> (Vec<Vec<PathNode>>, Vec<Vec<PathNode>>) {
    let (mut closed, mut open) = (Vec::new(), Vec::new());
    let mut current: Vec<PathNode> = Vec::new();
    let finish = |current: &mut Vec<PathNode>, open: &mut Vec<Vec<PathNode>>| {
        if current.len() >= 2 {
            open.push(std::mem::take(current));
        }
        current.clear();
    };
    for segment in segments {
        match segment {
            PathSegment::MoveTo(p) => {
                finish(&mut current, &mut open);
                current.push(corner(p));
            }
            PathSegment::LineTo(p) => current.push(corner(p)),
            PathSegment::QuadTo(q, p) => {
                // The cubic with the same shape as a quadratic has controls 2/3 of the way to it.
                let Some(from) = current.last().map(|n| Point::new(n.x, n.y)) else {
                    continue;
                };
                let lerp = |a: Point, b: SkPoint| {
                    point(SkPoint::from_xy(
                        (a.x + (b.x as f64 - a.x) * 2.0 / 3.0) as f32,
                        (a.y + (b.y as f64 - a.y) * 2.0 / 3.0) as f32,
                    ))
                };
                let to = point(p);
                curve(&mut current, lerp(from, q), lerp(to, q), p);
            }
            PathSegment::CubicTo(c1, c2, p) => curve(&mut current, point(c1), point(c2), p),
            PathSegment::Close => {
                // A closing point on top of the start folds into it.
                if current.len() > 2 {
                    let (first, last) = (current[0], current[current.len() - 1]);
                    if (first.x - last.x).abs() < 0.01 && (first.y - last.y).abs() < 0.01 {
                        current[0].handle_in = last.handle_in;
                        current.pop();
                    }
                }
                if current.len() >= 3 {
                    closed.push(std::mem::take(&mut current));
                }
                current.clear();
            }
        }
    }
    finish(&mut current, &mut open);
    (closed, open)
}

fn curve(current: &mut Vec<PathNode>, c1: Point, c2: Point, to: SkPoint) {
    if let Some(last) = current.last_mut() {
        last.handle_out = Some(c1);
    }
    current.push(PathNode {
        handle_in: Some(c2),
        ..corner(to)
    });
}

fn point(p: SkPoint) -> Point {
    Point::new(round(p.x as f64), round(p.y as f64))
}

fn corner(p: SkPoint) -> PathNode {
    PathNode::corner(round(p.x as f64), round(p.y as f64))
}

fn color(paint: &Paint) -> String {
    match paint {
        Paint::Color(c) => format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue),
        _ => "#888888".into(),
    }
}

fn round(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("i{n}")
        }
    }

    #[test]
    fn imports_shapes_with_transforms_and_styles() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <g transform="translate(10 20)">
                <rect x="0" y="0" width="30" height="40" fill="#ff0000" stroke="blue" stroke-width="3"/>
            </g>
            <circle cx="100" cy="50" r="20" fill="#00ff00"/>
            <path d="M0 0 Q 50 50 100 0" fill="none" stroke="#000"/>
        </svg>"##;
        let drawing = import_svg(svg, ids()).unwrap();
        assert_eq!((drawing.width, drawing.height), (200.0, 100.0));
        assert_eq!(drawing.shapes.len(), 3);

        let rect = &drawing.shapes[0];
        assert_eq!(rect.style.fill, "#ff0000");
        assert_eq!(rect.style.stroke, "#0000ff");
        assert_eq!(rect.style.stroke_width, 3.0);
        let Geometry::Path { nodes, closed, .. } = &rect.geometry else {
            panic!("path");
        };
        assert!(*closed);
        assert_eq!(nodes.len(), 4);
        assert_eq!((nodes[0].x, nodes[0].y), (10.0, 20.0), "moved by the group");

        let Geometry::Path { nodes, .. } = &drawing.shapes[1].geometry else {
            panic!("path");
        };
        assert!(
            nodes.iter().all(|n| n.handle_in.is_some()),
            "a circle is curves"
        );

        let open = &drawing.shapes[2];
        assert_eq!(open.style.fill, "none");
        let Geometry::Path { nodes, closed, .. } = &open.geometry else {
            panic!("path");
        };
        assert!(!closed);
        assert_eq!(nodes[0].handle_out, Some(Point::new(33.33, 33.33)));
    }

    #[test]
    fn keeps_holes_as_subpaths() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30">
            <path fill-rule="evenodd" d="M0 0H30V30H0Z M10 10H20V20H10Z"/>
        </svg>"#;
        let drawing = import_svg(svg, ids()).unwrap();
        let Geometry::Path { subpaths, .. } = &drawing.shapes[0].geometry else {
            panic!("path");
        };
        assert_eq!(subpaths.len(), 1);
    }

    #[test]
    fn reports_files_it_cannot_use() {
        assert!(import_svg("not svg", ids()).is_err());
        let empty = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#;
        assert_eq!(
            import_svg(empty, ids()).unwrap_err(),
            "no shapes found in this SVG"
        );
    }
}
