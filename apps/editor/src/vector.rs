//! The vector canvas: pen, rectangle, ellipse, line and select tools over an SVG drawing.

use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, PointerEvent};

use vellum_core::{find, insert};
use vellum_schema::{Geometry, Node, NodeKind, PathNode, Point, Shape, ShapeStyle};
use vellum_vector::{
    drawing_to_svg, ellipse_from_drag, is_meaningful, line_from_drag, path_data, pull_handles,
    rect_from_drag, translate,
};

use crate::shapes::shape_view;
use crate::store::{is_typing, new_id, use_store, Store, View};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Pen,
    Rect,
    Ellipse,
    Line,
}

const TOOLS: [(Tool, &str, &str); 5] = [
    (Tool::Select, "Select", "v"),
    (Tool::Pen, "Pen", "p"),
    (Tool::Rect, "Rectangle", "r"),
    (Tool::Ellipse, "Ellipse", "e"),
    (Tool::Line, "Line", "l"),
];

/// Distance (in drawing units) within which a pen click on the first anchor closes the path.
const CLOSE_RADIUS: f64 = 8.0;

#[derive(Clone)]
enum Gesture {
    Shape(Point),
    Move {
        id: String,
        start: Point,
        original: Geometry,
    },
    PenHandle(usize),
}

#[component]
pub fn VectorCanvas() -> impl IntoView {
    let store = use_store();
    let drawing = store.drawing;
    let tool = RwSignal::new(Tool::Pen);
    let style = RwSignal::new(ShapeStyle::default());
    let selected = RwSignal::new(None::<String>);
    let draft = RwSignal::new(None::<Shape>);
    let pen = RwSignal::new(Vec::<PathNode>::new());
    let cursor = RwSignal::new(None::<Point>);
    let gesture = StoredValue::new(None::<Gesture>);
    let svg_ref = NodeRef::<leptos::svg::Svg>::new();

    let selected_shape = move || {
        selected
            .get()
            .and_then(|id| drawing.with(|d| d.shapes.iter().find(|s| s.id == id).cloned()))
    };

    // Maps a pointer position to drawing units; the SVG keeps its aspect ratio, so one scale fits both axes.
    let to_local = move |ev: &PointerEvent| -> Point {
        let Some(svg) = svg_ref.get_untracked() else {
            return Point::default();
        };
        let r = svg.get_bounding_client_rect();
        let scale = drawing.with_untracked(|d| d.width) / r.width();
        Point::new(
            (ev.client_x() as f64 - r.left()) * scale,
            (ev.client_y() as f64 - r.top()) * scale,
        )
    };

    let add_shape = move |geometry: Geometry| {
        let style = style.get_untracked();
        drawing.update(|d| {
            d.shapes.push(Shape {
                id: new_id(),
                style,
                geometry,
            })
        });
    };

    let finish_pen = move |closed: bool| {
        let nodes = pen.get_untracked();
        if nodes.len() >= 2 {
            add_shape(Geometry::Path { nodes, closed });
        }
        pen.set(Vec::new());
    };

    let switch_tool = move |next: Tool| {
        if !pen.with_untracked(Vec::is_empty) {
            finish_pen(false);
        }
        tool.set(next);
        if next != Tool::Select {
            selected.set(None);
        }
    };

    let delete_selected = move || {
        if let Some(id) = selected.get_untracked() {
            drawing.update(|d| d.shapes.retain(|s| s.id != id));
            selected.set(None);
        }
    };

    let keys = window_event_listener(ev::keydown, move |ev| {
        if is_typing(&ev) || store.view.get_untracked() != View::Vector {
            return;
        }
        let key = ev.key();
        match key.as_str() {
            "Enter" | "Escape" => {
                if !pen.with_untracked(Vec::is_empty) {
                    finish_pen(false);
                } else if key == "Escape" {
                    selected.set(None);
                }
            }
            "Delete" | "Backspace" if selected.get_untracked().is_some() => {
                ev.prevent_default();
                delete_selected();
            }
            _ if !ev.meta_key() && !ev.ctrl_key() => {
                let key = key.to_lowercase();
                if let Some(&(t, _, _)) = TOOLS.iter().find(|(_, _, k)| *k == key) {
                    switch_tool(t);
                }
            }
            _ => {}
        }
    });
    on_cleanup(move || keys.remove());

    let on_pointer_down = move |ev: PointerEvent| {
        if ev.button() != 0 {
            return;
        }
        let p = to_local(&ev);
        let hit = ev
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|e| e.get_attribute("data-shape-id"));
        if let Some(el) = ev
            .current_target()
            .and_then(|t| t.dyn_into::<Element>().ok())
        {
            let _ = el.set_pointer_capture(ev.pointer_id());
        }

        match tool.get_untracked() {
            Tool::Select => {
                let shape = hit.and_then(|id| {
                    drawing.with_untracked(|d| d.shapes.iter().find(|s| s.id == id).cloned())
                });
                selected.set(shape.as_ref().map(|s| s.id.clone()));
                gesture.set_value(shape.map(|s| Gesture::Move {
                    id: s.id,
                    start: p,
                    original: s.geometry,
                }));
            }
            Tool::Pen => {
                let (count, first, last) =
                    pen.with_untracked(|ns| (ns.len(), ns.first().copied(), ns.last().copied()));
                if let Some(first) = first {
                    if count >= 2 && (p.x - first.x).hypot(p.y - first.y) <= CLOSE_RADIUS {
                        finish_pen(true);
                        return;
                    }
                }
                // The second click of a double-click lands on the last anchor; don't duplicate it.
                if last.is_some_and(|l| (p.x - l.x).hypot(p.y - l.y) < 3.0) {
                    return;
                }
                pen.update(|ns| ns.push(PathNode::corner(p.x, p.y)));
                gesture.set_value(Some(Gesture::PenHandle(count)));
            }
            _ => gesture.set_value(Some(Gesture::Shape(p))),
        }
    };

    let on_pointer_move = move |ev: PointerEvent| {
        let p = to_local(&ev);
        cursor.set(Some(p));
        match gesture.get_value() {
            Some(Gesture::Move {
                id,
                start,
                original,
            }) => drawing.update(|d| {
                if let Some(s) = d.shapes.iter_mut().find(|s| s.id == id) {
                    s.geometry = translate(&original, p.x - start.x, p.y - start.y);
                }
            }),
            Some(Gesture::PenHandle(i)) => pen.update(|ns| {
                if let Some(n) = ns.get_mut(i) {
                    *n = pull_handles(PathNode::corner(n.x, n.y), p);
                }
            }),
            Some(Gesture::Shape(start)) => {
                let geometry = match tool.get_untracked() {
                    Tool::Rect => rect_from_drag(start, p),
                    Tool::Ellipse => ellipse_from_drag(start, p),
                    _ => line_from_drag(start, p),
                };
                draft.set(Some(Shape {
                    id: "draft".into(),
                    style: style.get_untracked(),
                    geometry,
                }));
            }
            None => {}
        }
    };

    let on_pointer_up = move |_: PointerEvent| {
        if let Some(Gesture::Shape(_)) = gesture.get_value() {
            if let Some(shape) = draft.get_untracked() {
                if is_meaningful(&shape.geometry) {
                    add_shape(shape.geometry);
                }
            }
            draft.set(None);
        }
        gesture.set_value(None);
    };

    let update_style = move |f: &dyn Fn(&mut ShapeStyle)| match selected.get_untracked() {
        Some(id) => drawing.update(|d| {
            if let Some(s) = d.shapes.iter_mut().find(|s| s.id == id) {
                f(&mut s.style);
            }
        }),
        None => style.update(|s| f(s)),
    };
    let shown_style = move || {
        selected_shape()
            .map(|s| s.style)
            .unwrap_or_else(|| style.get())
    };

    let save_to_page = move |_| {
        let d = drawing.get_untracked();
        let target = store.editing_node.get_untracked().filter(|id| {
            store
                .doc
                .with_untracked(|doc| doc.pages.iter().any(|p| find(&p.nodes, id).is_some()))
        });
        match target {
            Some(id) => store.commit(|doc| {
                if let Some(n) = Store::find_mut(doc, &id) {
                    n.kind = NodeKind::Svg { drawing: d };
                }
            }),
            None => {
                let mut node = Node::new(new_id(), NodeKind::Svg { drawing: d });
                node.style.align = vellum_schema::Align::Center;
                store.editing_node.set(Some(node.id.clone()));
                store.commit(|doc| {
                    let nodes = store.nodes_mut(doc);
                    let end = nodes.len();
                    insert(nodes, None, end, node);
                });
            }
        }
        store.view.set(View::Page);
    };

    let export_svg = move |_| {
        let svg = drawing.with_untracked(drawing_to_svg);
        let _ = download("drawing.svg", "image/svg+xml", &svg);
    };

    let hint = move || {
        match tool.get() {
        Tool::Pen => "Click to add corners, drag to pull curves. Click the first point to close; Enter to finish.",
        Tool::Select => "Click a shape to select it, drag to move, Delete to remove.",
        _ => "Drag on the canvas to draw.",
    }
    };

    let tool_buttons = TOOLS
        .iter()
        .map(|&(t, label, key)| {
            view! {
                <button
                    type="button"
                    class="palette-item"
                    class:active=move || tool.get() == t
                    title=format!("{label} ({})", key.to_uppercase())
                    on:click=move |_| switch_tool(t)
                >
                    {label}
                    <kbd>{key.to_uppercase()}</kbd>
                </button>
            }
        })
        .collect_view();

    let pen_preview = move || {
        let nodes = pen.get();
        if nodes.is_empty() {
            return None;
        }
        let st = style.get();
        let last = *nodes.last().unwrap();
        let rubber = cursor
            .get()
            .filter(|_| gesture.with_value(Option::is_none))
            .map(|c| {
                view! { <line class="rubber-band" x1=last.x y1=last.y x2=c.x y2=c.y /> }
            });
        let handles = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let lines = [n.handle_in, n.handle_out].into_iter().flatten().map(|h| {
                    view! {
                        <line class="handle-line" x1=n.x y1=n.y x2=h.x y2=h.y />
                        <circle class="handle" cx=h.x cy=h.y r="3" />
                    }
                });
                view! {
                    {lines.collect_view()}
                    <rect class="anchor" class:first=i == 0 x=n.x - 4.0 y=n.y - 4.0 width="8" height="8" />
                }
            })
            .collect_view();
        Some(view! {
            <g class="pen-preview">
                <path d=path_data(&nodes, false) fill="none" stroke=st.stroke stroke-width=st.stroke_width />
                {rubber}
                {handles}
            </g>
        })
    };

    let selection_outline = move || {
        selected_shape().map(|s| {
            let outline = Shape {
                style: ShapeStyle {
                    fill: "none".into(),
                    stroke: "#2f80ff".into(),
                    stroke_width: 1.5,
                },
                ..s
            };
            view! { <g class="selection" pointer-events="none">{shape_view(&outline, false)}</g> }
        })
    };

    view! {
        <div class="workspace">
            <aside class="panel palette">
                <h2>"Tools"</h2>
                {tool_buttons}
                <p class="hint">{hint}</p>
            </aside>

            <main class="stage">
                <svg
                    node_ref=svg_ref
                    class=move || {
                        if tool.get() == Tool::Select { "vector-canvas tool-select" } else { "vector-canvas tool-draw" }
                    }
                    viewBox=move || drawing.with(|d| format!("0 0 {} {}", d.width, d.height))
                    on:pointerdown=on_pointer_down
                    on:pointermove=on_pointer_move
                    on:pointerup=on_pointer_up
                    on:pointerleave=move |_| cursor.set(None)
                    on:dblclick=move |_| {
                        if !pen.with_untracked(Vec::is_empty) {
                            finish_pen(false);
                        }
                    }
                >
                    <rect
                        width=move || drawing.with(|d| d.width)
                        height=move || drawing.with(|d| d.height)
                        fill="#fff"
                    />
                    {move || drawing.with(|d| d.shapes.iter().map(|s| shape_view(s, true)).collect_view())}
                    {move || draft.with(|d| d.as_ref().map(|s| shape_view(s, false)))}
                    {selection_outline}
                    {pen_preview}
                </svg>
            </main>

            <aside class="panel">
                <h3>{move || if selected.get().is_some() { "Selected shape" } else { "New shape style" }}</h3>
                <label class="field">
                    <span>"Fill"</span>
                    <input
                        type="color"
                        prop:value=move || shown_style().fill
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            update_style(&|s| s.fill = v.clone());
                        }
                    />
                </label>
                <label class="field">
                    <span>"Stroke"</span>
                    <input
                        type="color"
                        prop:value=move || shown_style().stroke
                        on:input=move |ev| {
                            let v = event_target_value(&ev);
                            update_style(&|s| s.stroke = v.clone());
                        }
                    />
                </label>
                <label class="field">
                    <span>"Stroke width (" {move || shown_style().stroke_width} ")"</span>
                    <input
                        type="range"
                        min="0"
                        max="24"
                        prop:value=move || shown_style().stroke_width.to_string()
                        on:input=move |ev| {
                            let v: f64 = event_target_value(&ev).parse().unwrap_or(0.0);
                            update_style(&|s| s.stroke_width = v);
                        }
                    />
                </label>
                <Show when=move || selected.get().is_some()>
                    <button type="button" class="danger" on:click=move |_| delete_selected()>
                        "Delete shape"
                    </button>
                </Show>
                <hr />
                <button
                    type="button"
                    class="primary"
                    disabled=move || drawing.with(|d| d.shapes.is_empty())
                    on:click=save_to_page
                >
                    {move || if store.editing_node.get().is_some() { "Save to page" } else { "Add to page" }}
                </button>
                <button type="button" disabled=move || drawing.with(|d| d.shapes.is_empty()) on:click=export_svg>
                    "Export SVG"
                </button>
                <Show when=move || store.editing_node.get().is_some()>
                    <button type="button" class="ghost" on:click=move |_| store.editing_node.set(None)>
                        "Detach from page block"
                    </button>
                </Show>
                <button
                    type="button"
                    class="ghost"
                    disabled=move || drawing.with(|d| d.shapes.is_empty())
                    on:click=move |_| {
                        drawing.update(|d| d.shapes.clear());
                        selected.set(None);
                    }
                >
                    "Clear canvas"
                </button>
            </aside>
        </div>
    }
}

/// Offers `contents` to the user as a file download.
fn download(filename: &str, mime: &str, contents: &str) -> Result<(), wasm_bindgen::JsValue> {
    let parts = js_sys::Array::of1(&contents.into());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let a: web_sys::HtmlAnchorElement = document().create_element("a")?.unchecked_into();
    a.set_href(&url);
    a.set_download(filename);
    a.click();
    web_sys::Url::revoke_object_url(&url)
}
