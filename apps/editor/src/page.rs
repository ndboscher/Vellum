//! The page editor: block palette, drag-and-drop page, and inspector.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{DragEvent, Element};

use vellum_core::{index_of, insert, move_to, new_node, nudge, remove, PALETTE};
use vellum_schema::{Align, Node, NodeId, NodeKind};

use crate::shapes::shape_view;
use crate::store::{new_id, use_store, Store, View};

const NEW_TYPE: &str = "application/x-vellum-new";
const MOVE_ID: &str = "application/x-vellum-move";

/// Whether a drag carries one of our payloads (and not, say, a file from the desktop).
fn is_our_drag(ev: &DragEvent) -> bool {
    ev.data_transfer().is_some_and(|dt| {
        let types = dt.types();
        types.includes(&NEW_TYPE.into(), 0) || types.includes(&MOVE_ID.into(), 0)
    })
}

/// True when the event fired on the element itself rather than bubbling from a child.
fn is_self(ev: &DragEvent) -> bool {
    ev.target() == ev.current_target()
}

#[component]
pub fn PageEditor() -> impl IntoView {
    let store = use_store();
    let selected = RwSignal::new(None::<NodeId>);
    let drop_index = RwSignal::new(None::<usize>);
    let len = move || store.doc.with_untracked(|d| d.nodes.len());

    let set_drop = move |i: Option<usize>| {
        if drop_index.get_untracked() != i {
            drop_index.set(i);
        }
    };

    let add = move |type_name: &str, at: usize| {
        if let Some(node) = new_node(type_name, new_id()) {
            let id = node.id.clone();
            store.commit(|d| insert(d, node, at));
            selected.set(Some(id));
        }
    };

    let on_drop = move |ev: DragEvent| {
        ev.prevent_default();
        let at = drop_index.get_untracked().unwrap_or_else(len);
        set_drop(None);
        let Some(dt) = ev.data_transfer() else { return };
        if let Ok(type_name) = dt.get_data(NEW_TYPE) {
            if !type_name.is_empty() {
                return add(&type_name, at);
            }
        }
        if let Ok(id) = dt.get_data(MOVE_ID) {
            if let Some(from) = store.doc.with_untracked(|d| index_of(d, &id)) {
                store.commit(|d| {
                    move_to(d, from, at);
                });
            }
        }
    };

    // Dragging over empty page space targets the end of the page.
    let over_empty = move |ev: DragEvent| {
        if is_our_drag(&ev) {
            ev.prevent_default();
            if is_self(&ev) {
                set_drop(Some(len()));
            }
        }
    };

    let palette = PALETTE
        .iter()
        .map(|&(type_name, label)| {
            view! {
                <button
                    type="button"
                    class="palette-item"
                    draggable="true"
                    on:dragstart=move |ev: DragEvent| {
                        if let Some(dt) = ev.data_transfer() {
                            let _ = dt.set_data(NEW_TYPE, type_name);
                            dt.set_effect_allowed("copy");
                        }
                    }
                    on:dragend=move |_| set_drop(None)
                    on:click=move |_| add(type_name, len())
                >
                    {label}
                </button>
            }
        })
        .collect_view();

    let block_row = move |i: usize, node: Node| {
        let id = node.id.clone();
        let id_sel = id.clone();
        let id_click = id.clone();
        view! {
            <div>
                {move || (drop_index.get() == Some(i)).then(|| view! { <div class="drop-line"></div> })}
                <div
                    class="block"
                    class:selected=move || selected.with(|s| s.as_deref() == Some(id_sel.as_str()))
                    draggable="true"
                    on:dragstart=move |ev: DragEvent| {
                        if let Some(dt) = ev.data_transfer() {
                            let _ = dt.set_data(MOVE_ID, &id);
                            dt.set_effect_allowed("move");
                        }
                    }
                    on:dragend=move |_| set_drop(None)
                    on:dragover=move |ev: DragEvent| {
                        if !is_our_drag(&ev) {
                            return;
                        }
                        ev.prevent_default();
                        ev.stop_propagation();
                        let Some(el) = ev.current_target().and_then(|t| t.dyn_into::<Element>().ok()) else {
                            return;
                        };
                        let r = el.get_bounding_client_rect();
                        let before = (ev.client_y() as f64) < r.top() + r.height() / 2.0;
                        set_drop(Some(if before { i } else { i + 1 }));
                    }
                    on:click=move |ev| {
                        ev.stop_propagation();
                        selected.set(Some(id_click.clone()));
                    }
                >
                    <span class="block-tag">{node.kind.type_name()}</span>
                    {block_view(&node)}
                </div>
            </div>
        }
    };

    view! {
        <div class="workspace">
            <aside class="panel palette">
                <h2>"Blocks"</h2>
                <p class="hint">"Drag onto the page, or click to append."</p>
                {palette}
            </aside>

            <main
                class="stage"
                on:click=move |_| selected.set(None)
                on:dragover=over_empty
                on:drop=on_drop
            >
                <div class="page" on:dragover=over_empty>
                    {move || {
                        store
                            .doc
                            .with(|d| d.nodes.is_empty())
                            .then(|| view! { <div class="empty">"Drop blocks here to start your page."</div> })
                    }}
                    {move || {
                        let nodes = store.doc.with(|d| d.nodes.clone());
                        nodes.into_iter().enumerate().map(|(i, n)| block_row(i, n)).collect_view()
                    }}
                    {move || {
                        let n = store.doc.with(|d| d.nodes.len());
                        (n > 0 && drop_index.get() == Some(n)).then(|| view! { <div class="drop-line"></div> })
                    }}
                </div>
            </main>

            <aside class="panel">
                <Inspector selected />
            </aside>
        </div>
    }
}

/// Renders a block as it will appear on the published page.
pub fn block_view(node: &Node) -> AnyView {
    let s = &node.style;
    let style = format!(
        "text-align:{};color:{};background:{};padding:{}px",
        s.align.as_css(),
        s.color,
        s.background.as_deref().unwrap_or("transparent"),
        s.padding
    );
    match &node.kind {
        NodeKind::Heading { text, level } => {
            let text = text.clone();
            match level {
                1 => view! { <h1 class="blk-heading" style=style>{text}</h1> }.into_any(),
                2 => view! { <h2 class="blk-heading" style=style>{text}</h2> }.into_any(),
                _ => view! { <h3 class="blk-heading" style=style>{text}</h3> }.into_any(),
            }
        }
        NodeKind::Text { text } => view! { <p class="blk-text" style=style>{text.clone()}</p> }.into_any(),
        NodeKind::Button { label, href } => view! {
            <div style=style>
                <a class="blk-button" href=href.clone() on:click=|ev| ev.prevent_default()>{label.clone()}</a>
            </div>
        }
        .into_any(),
        NodeKind::Image { src, alt } => view! {
            <div style=style>
                <img class="blk-image" src=src.clone() alt=alt.clone() draggable="false" />
            </div>
        }
        .into_any(),
        NodeKind::Spacer { height } => view! {
            <div class="blk-spacer" style=format!("height:{height}px;background:{}", s.background.as_deref().unwrap_or("transparent"))></div>
        }
        .into_any(),
        NodeKind::Svg { drawing } => {
            let shapes = drawing.shapes.iter().map(|sh| shape_view(sh, false)).collect_view();
            view! {
                <div style=style>
                    <svg
                        class="blk-vector"
                        viewBox=format!("0 0 {} {}", drawing.width, drawing.height)
                        role="img"
                        aria-label="Vector graphic"
                    >
                        {shapes}
                    </svg>
                </div>
            }
            .into_any()
        }
    }
}

/// Reads and writes one inspector value as text.
type Accessor = (fn(&Node) -> String, fn(&mut Node, String));

#[derive(Clone, Copy)]
enum Input {
    Text,
    TextArea,
    Color,
    Range(f64, f64),
    Select(&'static [(&'static str, &'static str)]),
}

/// One inspector control bound to a field of the node `id`. Values are read live from the
/// document, so the control stays in sync with undo/redo without being rebuilt.
fn field(
    store: Store,
    id: NodeId,
    label: &'static str,
    input: Input,
    get: fn(&Node) -> String,
    set: fn(&mut Node, String),
) -> AnyView {
    let id_get = id.clone();
    let value = move || {
        store.doc.with(|d| {
            d.nodes
                .iter()
                .find(|n| n.id == id_get)
                .map(get)
                .unwrap_or_default()
        })
    };
    let on_input = move |ev| {
        let v = event_target_value(&ev);
        store.edit(|d| {
            if let Some(n) = d.nodes.iter_mut().find(|n| n.id == id) {
                set(n, v);
            }
        });
    };
    let begin = move |_| store.begin_edit();

    let value_for_control = value.clone();
    let control = match input {
        Input::Text => view! { <input prop:value=value_for_control on:focus=begin on:input=on_input /> }.into_any(),
        Input::TextArea => {
            view! { <textarea rows="5" prop:value=value_for_control on:focus=begin on:input=on_input></textarea> }.into_any()
        }
        Input::Color => {
            view! { <input type="color" prop:value=value_for_control on:focus=begin on:input=on_input /> }.into_any()
        }
        Input::Range(min, max) => view! {
            <input type="range" min=min max=max prop:value=value_for_control on:pointerdown=move |_| store.begin_edit() on:focus=begin on:input=on_input />
        }
        .into_any(),
        Input::Select(options) => view! {
            <select prop:value=value_for_control on:focus=begin on:change=on_input>
                {options.iter().map(|&(v, l)| view! { <option value=v>{l}</option> }).collect_view()}
            </select>
        }
        .into_any(),
    };
    let shown = matches!(input, Input::Range(..)).then(|| value.clone());
    view! {
        <label class="field">
            <span>{label} {shown.map(|value| move || format!(" ({})", value()))}</span>
            {control}
        </label>
    }
    .into_any()
}

macro_rules! kind_field {
    ($variant:ident, $field:ident) => {
        (
            |n: &Node| match &n.kind {
                NodeKind::$variant { $field, .. } => $field.to_string(),
                _ => String::new(),
            },
            |n: &mut Node, v: String| {
                if let NodeKind::$variant { $field, .. } = &mut n.kind {
                    *$field = v.parse().unwrap_or_default();
                }
            },
        )
    };
}

/// Property panel for the selected block.
#[component]
fn Inspector(selected: RwSignal<Option<NodeId>>) -> impl IntoView {
    let store = use_store();
    // Rebuild only when the selection (or the selected block's existence) changes, not on every edit.
    let target = Memo::new(move |_| {
        let id = selected.get()?;
        store.doc.with(|d| {
            d.nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| (id.clone(), n.kind.type_name()))
        })
    });

    move || {
        let Some((id, type_name)) = target.get() else {
            return view! { <p class="hint">"Select a block to edit it."</p> }.into_any();
        };
        let f =
            |label, input, (get, set): Accessor| field(store, id.clone(), label, input, get, set);

        let content = match type_name {
            "heading" => view! {
                {f("Text", Input::Text, kind_field!(Heading, text))}
                {f("Level", Input::Select(&[("1", "H1"), ("2", "H2"), ("3", "H3")]), kind_field!(Heading, level))}
            }
            .into_any(),
            "text" => f("Text", Input::TextArea, kind_field!(Text, text)),
            "button" => view! {
                {f("Label", Input::Text, kind_field!(Button, label))}
                {f("Link", Input::Text, kind_field!(Button, href))}
            }
            .into_any(),
            "image" => view! {
                {f("Image URL", Input::Text, kind_field!(Image, src))}
                {f("Alt text", Input::Text, kind_field!(Image, alt))}
            }
            .into_any(),
            "spacer" => f("Height", Input::Range(8.0, 240.0), kind_field!(Spacer, height)),
            _ => {
                let id = id.clone();
                let open = move |_| {
                    let Some(drawing) = store.doc.with_untracked(|d| {
                        d.nodes.iter().find(|n| n.id == id).and_then(|n| match &n.kind {
                            NodeKind::Svg { drawing } => Some(drawing.clone()),
                            _ => None,
                        })
                    }) else {
                        return;
                    };
                    store.drawing.set(drawing);
                    store.editing_node.set(Some(id.clone()));
                    store.view.set(View::Vector);
                };
                view! {
                    <button type="button" class="primary" on:click=open>"Edit in vector canvas"</button>
                }
                .into_any()
            }
        };

        let text_style = (type_name != "spacer").then(|| {
            view! {
                {f(
                    "Align",
                    Input::Select(&[("left", "Left"), ("center", "Center"), ("right", "Right")]),
                    (
                        |n: &Node| n.style.align.as_css().to_string(),
                        |n: &mut Node, v: String| {
                            n.style.align = match v.as_str() {
                                "center" => Align::Center,
                                "right" => Align::Right,
                                _ => Align::Left,
                            }
                        },
                    ),
                )}
                {f("Text color", Input::Color, (|n: &Node| n.style.color.clone(), |n: &mut Node, v| n.style.color = v))}
                {f(
                    "Padding",
                    Input::Range(0.0, 96.0),
                    (|n: &Node| n.style.padding.to_string(), |n: &mut Node, v: String| n.style.padding = v.parse().unwrap_or(0.0)),
                )}
            }
        });

        let (id_clear, id_up, id_down, id_del) = (id.clone(), id.clone(), id.clone(), id.clone());
        view! {
            <div class="inspector">
                <h3>{type_name}</h3>
                {content}
                {text_style}
                {f(
                    "Background",
                    Input::Color,
                    (
                        |n: &Node| n.style.background.clone().unwrap_or_else(|| "#ffffff".into()),
                        |n: &mut Node, v| n.style.background = Some(v),
                    ),
                )}
                <button
                    type="button"
                    class="ghost"
                    on:click=move |_| {
                        store.commit(|d| {
                            if let Some(n) = d.nodes.iter_mut().find(|n| n.id == id_clear) {
                                n.style.background = None;
                            }
                        })
                    }
                >
                    "Clear background"
                </button>
                <div class="row actions">
                    <button type="button" title="Move up" on:click=move |_| store.commit(|d| { nudge(d, &id_up, -1); })>
                        "↑"
                    </button>
                    <button type="button" title="Move down" on:click=move |_| store.commit(|d| { nudge(d, &id_down, 1); })>
                        "↓"
                    </button>
                    <button
                        type="button"
                        class="danger"
                        on:click=move |_| {
                            store.commit(|d| {
                                remove(d, &id_del);
                            });
                            selected.set(None);
                        }
                    >
                        "Delete"
                    </button>
                </div>
            </div>
        }
        .into_any()
    }
}
