//! Union, subtract, intersect and exclude on filled shapes.
//!
//! Shapes are flattened to polygons (curves become short segments), combined with `geo`, and
//! the result comes back as one path whose extra outlines are subpaths filled even-odd.

use geo::{BooleanOps, Coord, LineString, MultiPolygon, Polygon, Simplify};
use vellum_schema::{Geometry, PathNode, Point, Shape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    /// Everything any shape covers.
    Union,
    /// The first (bottom) shape minus the others.
    Subtract,
    /// Only where every shape overlaps.
    Intersect,
    /// Where an odd number of shapes overlap.
    Exclude,
}

impl BooleanOp {
    pub const ALL: [BooleanOp; 4] = [
        BooleanOp::Union,
        BooleanOp::Subtract,
        BooleanOp::Intersect,
        BooleanOp::Exclude,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BooleanOp::Union => "Union",
            BooleanOp::Subtract => "Subtract",
            BooleanOp::Intersect => "Intersect",
            BooleanOp::Exclude => "Exclude",
        }
    }
}

/// Segments per curve or quarter ellipse when flattening.
const STEPS: usize = 16;

/// Drops points that move the outline less than this, in drawing units.
const TOLERANCE: f64 = 0.05;

/// Combines `shapes` (bottom first) into one path, or `None` if a shape has no area (a line
/// or open path) or nothing is left.
pub fn combine(shapes: &[&Shape], op: BooleanOp) -> Option<Geometry> {
    let (first, rest) = shapes.split_first()?;
    let mut acc = area(&first.geometry)?;
    for shape in rest {
        let other = area(&shape.geometry)?;
        acc = match op {
            BooleanOp::Union => acc.union(&other),
            BooleanOp::Subtract => acc.difference(&other),
            BooleanOp::Intersect => acc.intersection(&other),
            BooleanOp::Exclude => acc.xor(&other),
        };
    }
    to_path(&acc.simplify(TOLERANCE))
}

/// The area a shape fills, or `None` for lines and open paths.
pub fn area(geometry: &Geometry) -> Option<MultiPolygon<f64>> {
    let rings: Vec<Vec<Coord<f64>>> = match geometry {
        Geometry::Rect { x, y, w, h } => vec![vec![
            Coord { x: *x, y: *y },
            Coord { x: x + w, y: *y },
            Coord { x: x + w, y: y + h },
            Coord { x: *x, y: y + h },
        ]],
        Geometry::Ellipse { cx, cy, rx, ry } => {
            let n = STEPS * 4;
            vec![(0..n)
                .map(|i| {
                    let t = i as f64 / n as f64 * std::f64::consts::TAU;
                    Coord {
                        x: cx + rx * t.cos(),
                        y: cy + ry * t.sin(),
                    }
                })
                .collect()]
        }
        Geometry::Line { .. } => return None,
        Geometry::Path {
            nodes,
            closed,
            subpaths,
        } => {
            if !closed && subpaths.is_empty() {
                return None;
            }
            std::iter::once(nodes)
                .chain(subpaths)
                .map(|ring| flatten(ring))
                .collect()
        }
    };
    // Even-odd: each outline toggles what it covers.
    rings
        .into_iter()
        .filter(|ring| ring.len() >= 3)
        .map(|ring| MultiPolygon::new(vec![Polygon::new(LineString::new(ring), vec![])]))
        .reduce(|a, b| a.xor(&b))
}

/// The points along a closed pen outline, with curves split into segments.
fn flatten(nodes: &[PathNode]) -> Vec<Coord<f64>> {
    let mut out = Vec::new();
    for (i, from) in nodes.iter().enumerate() {
        let to = &nodes[(i + 1) % nodes.len()];
        out.push(Coord {
            x: from.x,
            y: from.y,
        });
        if from.handle_out.is_none() && to.handle_in.is_none() {
            continue;
        }
        let p0 = Point::new(from.x, from.y);
        let p1 = from.handle_out.unwrap_or(p0);
        let p3 = Point::new(to.x, to.y);
        let p2 = to.handle_in.unwrap_or(p3);
        for step in 1..STEPS {
            let t = step as f64 / STEPS as f64;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            out.push(Coord {
                x: a * p0.x + b * p1.x + c * p2.x + d * p3.x,
                y: a * p0.y + b * p1.y + c * p2.y + d * p3.y,
            });
        }
    }
    out
}

fn to_path(area: &MultiPolygon<f64>) -> Option<Geometry> {
    let ring = |line: &LineString<f64>| -> Vec<PathNode> {
        let mut coords: Vec<_> = line.coords().copied().collect();
        // geo repeats the first point at the end of a ring.
        if coords.len() > 1 && coords.first() == coords.last() {
            coords.pop();
        }
        coords
            .into_iter()
            .map(|c| PathNode::corner(round(c.x), round(c.y)))
            .collect()
    };
    let mut rings = area
        .0
        .iter()
        .flat_map(|p| std::iter::once(p.exterior()).chain(p.interiors()))
        .map(ring)
        .filter(|r| r.len() >= 3);
    let nodes = rings.next()?;
    Some(Geometry::Path {
        nodes,
        closed: true,
        subpaths: rings.collect(),
    })
}

fn round(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Area;
    use vellum_schema::ShapeStyle;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Shape {
        Shape::new("r", ShapeStyle::default(), Geometry::Rect { x, y, w, h })
    }

    fn size(g: &Geometry) -> f64 {
        area(g).unwrap().unsigned_area()
    }

    #[test]
    fn combines_overlapping_squares() {
        let (a, b) = (rect(0.0, 0.0, 10.0, 10.0), rect(5.0, 5.0, 10.0, 10.0));
        let area_of = |op| size(&combine(&[&a, &b], op).unwrap());
        assert_eq!(area_of(BooleanOp::Union), 175.0);
        assert_eq!(area_of(BooleanOp::Subtract), 75.0);
        assert_eq!(area_of(BooleanOp::Intersect), 25.0);
        assert_eq!(area_of(BooleanOp::Exclude), 150.0);
    }

    #[test]
    fn subtracting_from_the_middle_leaves_a_hole() {
        let (outer, inner) = (rect(0.0, 0.0, 30.0, 30.0), rect(10.0, 10.0, 10.0, 10.0));
        let Some(ring) = combine(&[&outer, &inner], BooleanOp::Subtract) else {
            panic!("a ring");
        };
        let Geometry::Path { subpaths, .. } = &ring else {
            panic!("a path");
        };
        assert_eq!(subpaths.len(), 1, "the hole");
        assert_eq!(size(&ring), 800.0);
        assert!(ring.is_filled());
    }

    #[test]
    fn curves_and_ellipses_flatten_closely() {
        let circle = Shape::new(
            "c",
            ShapeStyle::default(),
            Geometry::Ellipse {
                cx: 0.0,
                cy: 0.0,
                rx: 10.0,
                ry: 10.0,
            },
        );
        let unioned = combine(&[&circle], BooleanOp::Union).unwrap();
        let exact = std::f64::consts::PI * 100.0;
        assert!((size(&unioned) - exact).abs() / exact < 0.01);
    }

    #[test]
    fn refuses_shapes_without_area_and_empty_results() {
        let line = Shape::new(
            "l",
            ShapeStyle::default(),
            Geometry::Line {
                x1: 0.0,
                y1: 0.0,
                x2: 5.0,
                y2: 5.0,
            },
        );
        let a = rect(0.0, 0.0, 10.0, 10.0);
        assert!(combine(&[&a, &line], BooleanOp::Union).is_none());
        let far = rect(50.0, 50.0, 10.0, 10.0);
        assert!(combine(&[&a, &far], BooleanOp::Intersect).is_none());
    }
}
