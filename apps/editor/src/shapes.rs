//! SVG rendering of vector shapes, shared by the canvas and page blocks.

use leptos::prelude::*;
use vellum_schema::{Geometry, Shape};
use vellum_vector::path_data;

/// Renders a shape. `hit` tags it with `data-shape-id` so the select tool can find it.
pub fn shape_view(shape: &Shape, hit: bool) -> AnyView {
    let fill = if shape.geometry.is_filled() {
        shape.style.fill.clone()
    } else {
        "none".into()
    };
    let stroke = shape.style.stroke.clone();
    let width = shape.style.stroke_width;
    let id = hit.then(|| shape.id.clone());
    match &shape.geometry {
        Geometry::Rect { x, y, w, h } => view! {
            <rect x=*x y=*y width=*w height=*h fill=fill stroke=stroke stroke-width=width data-shape-id=id />
        }
        .into_any(),
        Geometry::Ellipse { cx, cy, rx, ry } => view! {
            <ellipse cx=*cx cy=*cy rx=*rx ry=*ry fill=fill stroke=stroke stroke-width=width data-shape-id=id />
        }
        .into_any(),
        Geometry::Line { x1, y1, x2, y2 } => view! {
            <line
                x1=*x1 y1=*y1 x2=*x2 y2=*y2
                stroke=stroke stroke-width=width stroke-linecap="round" data-shape-id=id
            />
        }
        .into_any(),
        Geometry::Path { nodes, closed } => view! {
            <path
                d=path_data(nodes, *closed)
                fill=fill stroke=stroke stroke-width=width
                stroke-linejoin="round" stroke-linecap="round" data-shape-id=id
            />
        }
        .into_any(),
    }
}
