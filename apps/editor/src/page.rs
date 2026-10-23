//! The page editor: block palette, drag-and-drop page tree, and inspector.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{DragEvent, Element};

use vellum_core::{
    children, create_component, find, find_mut, insert, locate, move_node, new_instance, new_node,
    nudge, remove, would_cycle, PALETTE,
};
use vellum_schema::{
    css, parse_token_ref, token_ref, Align, AlignItems, Direction, Justify, Layout, LayoutMode,
    Length, Node, NodeId, NodeKind, TokenKind,
};

use crate::shapes::shape_view;
use crate::site::{
    binding_fields, instance_fields, override_fields, preview_width, BreakpointBar,
    ComponentBanner, PagesPanel,
};
use crate::store::{new_id, use_store, Store, View};

const NEW_TYPE: &str = "application/x-vellum-new";
const MOVE_ID: &str = "application/x-vellum-move";
/// A `NEW_TYPE` payload starting with this places a component: `component:<id>`.
const COMPONENT_PREFIX: &str = "component:";

/// Distance in px from a container's edge within which a drop lands beside it, not inside.
const EDGE: f64 = 14.0;

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

/// An insertion point: index `at` in `parent`'s children (`None` is the page's top level).
#[derive(Clone, Debug, PartialEq)]
struct DropTarget {
    parent: Option<NodeId>,
    at: usize,
}

/// State shared by every block on the canvas.
#[derive(Clone, Copy)]
struct Canvas {
    store: Store,
    selected: RwSignal<Option<NodeId>>,
    drop: RwSignal<Option<DropTarget>>,
}

impl Canvas {
    fn set_drop(&self, target: Option<DropTarget>) {
        if self.drop.with_untracked(|d| *d != target) {
            self.drop.set(target);
        }
    }

    fn top_level_end(&self) -> DropTarget {
        DropTarget {
            parent: None,
            at: self.store.doc.with_untracked(|d| self.store.nodes(d).len()),
        }
    }

    /// Where a clicked palette item goes: inside the selected container, right after the
    /// selected block, or at the end of the page.
    fn click_target(&self) -> DropTarget {
        let Some(id) = self.selected.get_untracked() else {
            return self.top_level_end();
        };
        self.store
            .doc
            .with_untracked(|d| {
                let nodes = self.store.nodes(d);
                if let Some(len) = children(nodes, Some(&id)).map(<[Node]>::len) {
                    return Some(DropTarget {
                        parent: Some(id),
                        at: len,
                    });
                }
                let (parent, i) = locate(nodes, &id)?;
                Some(DropTarget { parent, at: i + 1 })
            })
            .unwrap_or_else(|| self.top_level_end())
    }

    /// Adds a palette block, or with `component:<id>` an instance of that component (unless
    /// the open component would then contain itself).
    fn add(&self, type_name: &str, target: DropTarget) {
        let node = match type_name.strip_prefix(COMPONENT_PREFIX) {
            Some(component) => self.store.doc.with_untracked(|d| {
                let tree = self.store.open_tree(d);
                if would_cycle(d, &tree, component) {
                    None
                } else {
                    new_instance(d, component, new_id())
                }
            }),
            None => new_node(type_name, new_id()),
        };
        if let Some(node) = node {
            let id = node.id.clone();
            self.store.commit(|d| {
                insert(
                    self.store.nodes_mut(d),
                    target.parent.as_deref(),
                    target.at,
                    node,
                );
            });
            self.selected.set(Some(id));
        }
    }

    fn is_drop(&self, parent: &Option<NodeId>, at: usize) -> bool {
        self.drop.with(|d| {
            d.as_ref()
                .is_some_and(|d| d.parent == *parent && d.at == at)
        })
    }
}

#[component]
pub fn PageEditor() -> impl IntoView {
    let store = use_store();
    let c = Canvas {
        store,
        selected: RwSignal::new(None),
        drop: RwSignal::new(None),
    };

    let on_drop = move |ev: DragEvent| {
        ev.prevent_default();
        let target = c.drop.get_untracked().unwrap_or_else(|| c.top_level_end());
        c.set_drop(None);
        let Some(dt) = ev.data_transfer() else { return };
        if let Ok(type_name) = dt.get_data(NEW_TYPE) {
            if !type_name.is_empty() {
                return c.add(&type_name, target);
            }
        }
        if let Ok(id) = dt.get_data(MOVE_ID) {
            if !id.is_empty() {
                store.commit(|d| {
                    move_node(store.nodes_mut(d), &id, target.parent.as_deref(), target.at);
                });
            }
        }
    };

    // Dragging over empty page space targets the end of the page.
    let over_empty = move |ev: DragEvent| {
        if is_our_drag(&ev) {
            ev.prevent_default();
            if is_self(&ev) {
                c.set_drop(Some(c.top_level_end()));
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
                    on:dragend=move |_| c.set_drop(None)
                    on:click=move |_| c.add(type_name, c.click_target())
                >
                    {label}
                </button>
            }
        })
        .collect_view();

    // Components can be placed like blocks, except inside themselves.
    let component_palette = move || {
        let open = store.page.get();
        store.doc.with(|d| {
            d.components
                .iter()
                .filter(|comp| !would_cycle(d, &open, &comp.id))
                .map(|comp| {
                    let payload = format!("{COMPONENT_PREFIX}{}", comp.id);
                    let payload_click = payload.clone();
                    view! {
                        <button
                            type="button"
                            class="palette-item component-item"
                            draggable="true"
                            on:dragstart=move |ev: DragEvent| {
                                if let Some(dt) = ev.data_transfer() {
                                    let _ = dt.set_data(NEW_TYPE, &payload);
                                    dt.set_effect_allowed("copy");
                                }
                            }
                            on:dragend=move |_| c.set_drop(None)
                            on:click=move |_| c.add(&payload_click, c.click_target())
                        >
                            {comp.name.clone()}
                            <kbd>"◆"</kbd>
                        </button>
                    }
                })
                .collect_view()
        })
    };

    let page_vars = move || {
        let dark = store.dark_preview.get();
        let width = preview_width(store.breakpoint.get());
        store.doc.with(|d| inline_style(d.tokens.variables(dark))) + width
    };

    view! {
        <div class="workspace">
            <aside class="panel palette">
                <PagesPanel selected=c.selected />
                <h2>"Blocks"</h2>
                <p class="hint">"Drag onto the page or into a Stack, Row or Grid. Click to add next to the selected block."</p>
                {palette}
                {component_palette}
            </aside>

            <main
                class="stage"
                on:click=move |_| c.selected.set(None)
                on:dragover=over_empty
                on:drop=on_drop
            >
                <div class="stage-inner">
                <ComponentBanner selected=c.selected />
                <BreakpointBar />
                <div class="page" class:dark=move || store.dark_preview.get() style=page_vars on:dragover=over_empty>
                    {move || {
                        store.page.track();
                        store
                            .doc
                            .with(|d| store.nodes(d).is_empty())
                            .then(|| view! { <div class="empty">"Drop blocks here to start your page."</div> })
                    }}
                    {move || {
                        store.page.track();
                        store.breakpoint.track();
                        let nodes = store.doc.with(|d| store.nodes(d).clone());
                        block_list(c, None, false, nodes)
                    }}
                </div>
                </div>
            </main>

            <aside class="panel">
                <Inspector selected=c.selected />
            </aside>
        </div>
    }
}

fn block_list(c: Canvas, parent: Option<NodeId>, horizontal: bool, nodes: Vec<Node>) -> AnyView {
    let last = nodes.len().saturating_sub(1);
    nodes
        .into_iter()
        .enumerate()
        .map(|(i, n)| block_row(c, parent.clone(), horizontal, i, i == last, n))
        .collect_view()
        .into_any()
}

/// One draggable, selectable block. Containers render their children inside.
fn block_row(
    c: Canvas,
    parent: Option<NodeId>,
    horizontal: bool,
    i: usize,
    is_last: bool,
    node: Node,
) -> AnyView {
    let id = node.id.clone();
    let (id_sel, id_click, id_into, id_drag) = (id.clone(), id.clone(), id.clone(), id.clone());
    let (p_before, p_after, p_over) = (parent.clone(), parent.clone(), parent);
    let child_count = node.children().map(Vec::len);
    let hidden_here = node.override_at(c.store.breakpoint.get_untracked()).hidden;

    let on_dragover = move |ev: DragEvent| {
        if !is_our_drag(&ev) {
            return;
        }
        ev.prevent_default();
        ev.stop_propagation();
        let Some(el) = ev
            .current_target()
            .and_then(|t| t.dyn_into::<Element>().ok())
        else {
            return;
        };
        let r = el.get_bounding_client_rect();
        let (pos, start, size) = if horizontal {
            (ev.client_x() as f64, r.left(), r.width())
        } else {
            (ev.client_y() as f64, r.top(), r.height())
        };
        let beside = |at| DropTarget {
            parent: p_over.clone(),
            at,
        };
        let target = match child_count {
            // Over a container's body: append inside it. Near its edges: drop beside it.
            Some(len) if pos > start + EDGE && pos < start + size - EDGE => DropTarget {
                parent: Some(id_drag.clone()),
                at: len,
            },
            _ if pos < start + size / 2.0 => beside(i),
            _ => beside(i + 1),
        };
        c.set_drop(Some(target));
    };

    view! {
        <div
            class="block"
            class:horizontal=horizontal
            class:hidden-here=hidden_here
            title=hidden_here.then_some("Hidden at this breakpoint")
            class:selected=move || c.selected.with(|s| s.as_deref() == Some(id_sel.as_str()))
            class:drop-before=move || c.is_drop(&p_before, i)
            class:drop-after=move || is_last && c.is_drop(&p_after, i + 1)
            class:drop-into=move || c.drop.with(|d| d.as_ref().is_some_and(|d| d.parent.as_deref() == Some(id_into.as_str())))
            draggable="true"
            on:dragstart=move |ev: DragEvent| {
                // Nested blocks sit inside draggable containers; only the innermost one moves.
                ev.stop_propagation();
                if let Some(dt) = ev.data_transfer() {
                    let _ = dt.set_data(MOVE_ID, &id);
                    dt.set_effect_allowed("move");
                }
            }
            on:dragend=move |_| c.set_drop(None)
            on:dragover=on_dragover
            on:click=move |ev| {
                ev.stop_propagation();
                c.selected.set(Some(id_click.clone()));
            }
        >
            <span class="block-tag">{tag_label(&node)}</span>
            {block_view(c, &node, true)}
        </div>
    }
    .into_any()
}

fn tag_label(node: &Node) -> &'static str {
    match &node.kind {
        NodeKind::Container { layout, .. } => layout.label(),
        NodeKind::Instance { .. } => "component",
        kind => kind.type_name(),
    }
}

/// Joins declarations into an inline `style`, dropping values that could break out of it,
/// as the exporters drop them from their stylesheets.
fn inline_style<P: AsRef<str>>(declarations: Vec<(P, String)>) -> String {
    declarations
        .into_iter()
        .filter(|(_, v)| !v.trim().is_empty() && !v.contains([';', '{', '}', '<', '>', '\\']))
        .map(|(p, v)| format!("{}:{v}", p.as_ref()))
        .collect::<Vec<_>>()
        .join(";")
}

/// Renders a block as it will appear on the published page.
///
/// Blocks inside a component instance are not `interactive`: they show the component's look
/// but are edited in the component itself.
fn block_view(c: Canvas, node: &Node, interactive: bool) -> AnyView {
    // What the block looks like at the previewed breakpoint.
    let o = node.override_at(c.store.breakpoint.get_untracked());
    let s = &o.apply_style(&node.style);
    let style = inline_style(s.declarations());
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
        NodeKind::Spacer { height } => {
            let background = s.background.as_deref().map_or("transparent".into(), css::color);
            let style = inline_style(vec![("height", format!("{}px", css::number(*height))), ("background", background)]);
            view! { <div class="blk-spacer" style=style></div> }.into_any()
        }
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
        NodeKind::Container { layout, children } => {
            let layout = &o.apply_layout(layout);
            let mut declarations = s.declarations();
            declarations.extend(layout.declarations());
            let empty = children.is_empty().then(|| {
                view! { <div class="container-empty">"Drop blocks here"</div> }
            });
            view! {
                <div class="blk-container" style=inline_style(declarations)>
                    {empty}
                    {if interactive {
                        block_list(c, Some(node.id.clone()), layout.is_horizontal(), children.clone())
                    } else {
                        children.iter().map(|n| block_view(c, n, false)).collect_view().into_any()
                    }}
                </div>
            }
            .into_any()
        }
        NodeKind::Nav {} => {
            let current = c.store.page.get_untracked();
            let links = c.store.doc.with_untracked(|d| {
                let current = &d.page_or_home(&current).id;
                d.pages
                    .iter()
                    .map(|p| {
                        let here = (&p.id == current).then_some("page");
                        view! {
                            <a class="blk-nav-link" href=p.path.clone() aria-current=here on:click=|ev| ev.prevent_default()>
                                {p.name.clone()}
                            </a>
                        }
                    })
                    .collect_view()
            });
            view! { <nav class="blk-nav" style=style>{links}</nav> }.into_any()
        }
        NodeKind::Instance { component, props } => {
            let expanded = c.store.doc.with_untracked(|d| {
                d.component(component).map(|comp| vellum_core::expand(comp, props))
            });
            // Cycle checks keep components from holding themselves; a hand-edited file that
            // does anyway stops here instead of recursing forever.
            let depth = INSTANCE_DEPTH.with(|d| d.get());
            let inner = match expanded {
                Some(nodes) if depth < 8 => {
                    INSTANCE_DEPTH.with(|d| d.set(depth + 1));
                    let view = nodes.iter().map(|n| block_view(c, n, false)).collect_view().into_any();
                    INSTANCE_DEPTH.with(|d| d.set(depth));
                    view
                }
                Some(_) => ().into_any(),
                None => view! { <div class="empty">"Missing component"</div> }.into_any(),
            };
            view! { <div class="blk-component">{inner}</div> }.into_any()
        }
    }
}

thread_local! {
    static INSTANCE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Reads and writes one inspector value as text.
type Accessor = (fn(&Node) -> String, fn(&mut Node, String));

#[derive(Clone, Copy)]
enum Input {
    Text,
    TextArea,
    Range(f64, f64),
    Select(&'static [(&'static str, &'static str)]),
}

/// Reads a value of node `id` live from the document, so controls follow undo/redo.
fn read<T: Default>(store: Store, id: &str, f: impl Fn(&Node) -> T) -> T {
    store
        .doc
        .with(|d| find(store.nodes(d), id).map(f).unwrap_or_default())
}

/// One step of a continuous edit (typing, dragging a slider) on node `id`.
fn edit(store: Store, id: &str, f: impl FnOnce(&mut Node)) {
    store.edit(|d| {
        if let Some(n) = find_mut(store.nodes_mut(d), id) {
            f(n);
        }
    });
}

/// A discrete change (picking from a list) on node `id`, as its own undo step.
fn commit(store: Store, id: &str, f: impl FnOnce(&mut Node)) {
    store.commit(|d| {
        if let Some(n) = find_mut(store.nodes_mut(d), id) {
            f(n);
        }
    });
}

/// One inspector control bound to a field of the node `id`.
fn field(
    store: Store,
    id: NodeId,
    label: &'static str,
    input: Input,
    get: fn(&Node) -> String,
    set: fn(&mut Node, String),
) -> AnyView {
    let id_get = id.clone();
    let value = move || read(store, &id_get, get);
    let on_input = move |ev| {
        let v = event_target_value(&ev);
        edit(store, &id, |n| set(n, v));
    };
    let begin = move |_| store.begin_edit();

    let value_for_control = value.clone();
    let control = match input {
        Input::Text => view! { <input prop:value=value_for_control on:focus=begin on:input=on_input /> }.into_any(),
        Input::TextArea => {
            view! { <textarea rows="5" prop:value=value_for_control on:focus=begin on:input=on_input></textarea> }.into_any()
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

const CUSTOM: &str = "__custom";
const NONE: &str = "__none";

/// The names of one kind of token, read live, as `<option>`s.
fn token_options(store: Store, kind: TokenKind) -> impl IntoView {
    move || {
        store.doc.with(|d| {
            d.tokens
                .names(kind)
                .into_iter()
                .map(|name| {
                    let name = name.to_owned();
                    view! { <option value=name.clone()>{name.clone()}</option> }
                })
                .collect_view()
        })
    }
}

/// A color picked from the color tokens, or a custom color. `get`/`set` use `None` for
/// "no color", which is offered only when `optional`.
fn color_field(
    store: Store,
    id: NodeId,
    label: &'static str,
    optional: bool,
    get: fn(&Node) -> Option<String>,
    set: fn(&mut Node, Option<String>),
) -> AnyView {
    let id_value = id.clone();
    let value = move || read(store, &id_value, get);
    let choice = {
        let value = value.clone();
        move || match value() {
            None => NONE.to_owned(),
            Some(v) => parse_token_ref(&v).map_or(CUSTOM.to_owned(), str::to_owned),
        }
    };
    let id_pick = id.clone();
    let on_pick = move |ev| {
        let picked = event_target_value(&ev);
        commit(store, &id_pick, |n| {
            let next = match picked.as_str() {
                NONE => None,
                // Keep the current look: start a custom color from the resolved token value.
                CUSTOM => Some(resolve_color(store, get(n).as_deref())),
                name => Some(token_ref(name)),
            };
            set(n, next);
        });
    };
    let is_custom = {
        let choice = choice.clone();
        move || choice() == CUSTOM
    };
    let custom_value = move || {
        value()
            .filter(|v| parse_token_ref(v).is_none())
            .unwrap_or_default()
    };
    let on_color = move |ev| {
        let v = event_target_value(&ev);
        edit(store, &id, |n| set(n, Some(v)));
    };
    view! {
        <label class="field">
            <span>{label}</span>
            <div class="row">
                <select prop:value=choice on:change=on_pick>
                    {optional.then(|| view! { <option value=NONE>"None"</option> })}
                    {token_options(store, TokenKind::Color)}
                    <option value=CUSTOM>"Custom…"</option>
                </select>
                <Show when=is_custom>
                    <input type="color" prop:value=custom_value.clone() on:focus=move |_| store.begin_edit() on:input=on_color.clone() />
                </Show>
            </div>
        </label>
    }
    .into_any()
}

/// The literal value a color resolves to with light tokens, as a `#rrggbb` the picker accepts.
fn resolve_color(store: Store, value: Option<&str>) -> String {
    let literal = match value.and_then(parse_token_ref) {
        Some(name) => store.doc.with_untracked(|d| {
            d.tokens
                .colors
                .iter()
                .find(|t| t.name == name)
                .map(|t| t.light.clone())
        }),
        None => value.map(str::to_owned),
    };
    literal
        .filter(|v| v.starts_with('#') && v.len() == 7)
        .unwrap_or_else(|| "#1f1b2e".into())
}

/// A size picked from the spacing tokens, or a custom number of px.
fn length_field(
    store: Store,
    id: NodeId,
    label: &'static str,
    max: f64,
    get: fn(&Node) -> Length,
    set: fn(&mut Node, Length),
) -> AnyView {
    let id_value = id.clone();
    let value = move || read(store, &id_value, get);
    let choice = {
        let value = value.clone();
        move || match value() {
            Length::Token(name) => name,
            Length::Px(_) => CUSTOM.to_owned(),
        }
    };
    let id_pick = id.clone();
    let on_pick = move |ev| {
        let picked = event_target_value(&ev);
        commit(store, &id_pick, |n| {
            let next = if picked == CUSTOM {
                let px = match get(n) {
                    Length::Px(px) => px,
                    Length::Token(name) => store.doc.with_untracked(|d| {
                        d.tokens
                            .spacing
                            .iter()
                            .find(|t| t.name == name)
                            .map_or(0.0, |t| t.value)
                    }),
                };
                Length::Px(px)
            } else {
                Length::Token(picked)
            };
            set(n, next);
        });
    };
    let px = Signal::derive(move || match value() {
        Length::Px(px) => Some(px),
        Length::Token(_) => None,
    });
    let on_px = move |ev| {
        let v = event_target_value(&ev).parse().unwrap_or(0.0);
        edit(store, &id, |n| set(n, Length::Px(v)));
    };
    let is_custom = move || px.get().is_some();
    view! {
        <label class="field">
            <span>{label} {move || px.get().map(|v| format!(" ({v}px)"))}</span>
            <div class="row">
                <select prop:value=choice on:change=on_pick>
                    {token_options(store, TokenKind::Space)}
                    <option value=CUSTOM>"Custom…"</option>
                </select>
                <Show when=is_custom>
                    <input
                        type="range"
                        min="0"
                        max=max
                        prop:value=move || px.get().unwrap_or_default()
                        on:pointerdown=move |_| store.begin_edit()
                        on:focus=move |_| store.begin_edit()
                        on:input=on_px.clone()
                    />
                </Show>
            </div>
        </label>
    }
    .into_any()
}

fn font_field(store: Store, id: NodeId) -> AnyView {
    let id_value = id.clone();
    let value =
        move || read(store, &id_value, |n| n.style.font.clone()).unwrap_or_else(|| NONE.into());
    let on_pick = move |ev| {
        let picked = event_target_value(&ev);
        commit(store, &id, |n| {
            n.style.font = (picked != NONE).then_some(picked)
        });
    };
    view! {
        <label class="field">
            <span>"Type"</span>
            <select prop:value=value on:change=on_pick>
                <option value=NONE>"Inherit"</option>
                {token_options(store, TokenKind::Font)}
            </select>
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

/// Reads and writes one field of a container's layout as text.
macro_rules! layout_field {
    ($field:ident, $to:expr, $from:expr) => {
        (
            |n: &Node| match &n.kind {
                NodeKind::Container { layout, .. } => ($to)(&layout.$field),
                _ => String::new(),
            },
            |n: &mut Node, v: String| {
                if let NodeKind::Container { layout, .. } = &mut n.kind {
                    if let Some(v) = ($from)(v.as_str()) {
                        layout.$field = v;
                    }
                }
            },
        )
    };
}

fn layout_of(n: &Node) -> Option<&Layout> {
    match &n.kind {
        NodeKind::Container { layout, .. } => Some(layout),
        _ => None,
    }
}

fn layout_mut(n: &mut Node) -> Option<&mut Layout> {
    match &mut n.kind {
        NodeKind::Container { layout, .. } => Some(layout),
        _ => None,
    }
}

/// Controls for a container's layout. Flex-only and grid-only controls follow the mode.
fn layout_fields(store: Store, id: NodeId) -> AnyView {
    let f = |label, input, (get, set): Accessor| field(store, id.clone(), label, input, get, set);
    let id_mode = id.clone();
    let is_grid = move || {
        read(store, &id_mode, |n| {
            layout_of(n).is_some_and(|l| l.mode == LayoutMode::Grid)
        })
    };
    let is_flex = {
        let is_grid = is_grid.clone();
        move || !is_grid()
    };
    let grid = f(
        "Columns",
        Input::Range(1.0, 6.0),
        layout_field!(columns, |c: &u8| c.to_string(), |v: &str| v
            .parse::<u8>()
            .ok()),
    );
    let flex = view! {
        {f(
            "Direction",
            Input::Select(&[("row", "Row"), ("column", "Column")]),
            layout_field!(direction, |d: &Direction| d.as_str().to_owned(), Direction::parse),
        )}
        {f(
            "Wrap",
            Input::Select(&[("true", "Wrap onto new lines"), ("false", "Keep on one line")]),
            layout_field!(wrap, |w: &bool| w.to_string(), |v: &str| v.parse::<bool>().ok()),
        )}
        {f(
            "Justify",
            Input::Select(&[("start", "Start"), ("center", "Center"), ("end", "End"), ("between", "Space between")]),
            layout_field!(justify, |j: &Justify| j.as_str().to_owned(), Justify::parse),
        )}
    };
    view! {
        {f(
            "Layout",
            Input::Select(&[("flex", "Flex"), ("grid", "Grid")]),
            layout_field!(mode, |m: &LayoutMode| m.as_str().to_owned(), LayoutMode::parse),
        )}
        <div class="fields" class:hidden=is_grid.clone()>{flex}</div>
        <div class="fields" class:hidden=is_flex>{grid}</div>
        {length_field(
            store,
            id.clone(),
            "Gap",
            96.0,
            |n| layout_of(n).map(|l| l.gap.clone()).unwrap_or_default(),
            |n, v| {
                if let Some(l) = layout_mut(n) {
                    l.gap = v;
                }
            },
        )}
        {f(
            "Align items",
            Input::Select(&[("stretch", "Stretch"), ("start", "Start"), ("center", "Center"), ("end", "End")]),
            layout_field!(align, |a: &AlignItems| a.as_str().to_owned(), AlignItems::parse),
        )}
    }
    .into_any()
}

/// Property panel for the selected block.
#[component]
fn Inspector(selected: RwSignal<Option<NodeId>>) -> impl IntoView {
    let store = use_store();
    // Rebuild only when the selection (or the selected block's existence) changes, not on every edit.
    let target = Memo::new(move |_| {
        let id = selected.get()?;
        store.page.track();
        store
            .doc
            .with(|d| find(store.nodes(d), &id).map(|n| (id.clone(), n.kind.type_name())))
    });

    move || {
        let Some((id, type_name)) = target.get() else {
            return view! { <p class="hint">"Select a block to edit it."</p> }.into_any();
        };
        let breakpoint = store.breakpoint.get();
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
            "container" => layout_fields(store, id.clone()),
            "instance" => instance_fields(store, id.clone(), selected),
            "nav" => view! {
                <p class="hint">"Links to every page, in the order listed under Pages. The open page's link is highlighted."</p>
            }
            .into_any(),
            _ => {
                let id = id.clone();
                let open = move |_| {
                    let Some(drawing) = store.doc.with_untracked(|d| {
                        find(store.nodes(d), &id).and_then(|n| match &n.kind {
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

        let is_instance = type_name == "instance";
        let text_style = (type_name != "spacer" && !is_instance).then(|| {
            view! {
                <hr />
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
                {font_field(store, id.clone())}
                {color_field(store, id.clone(), "Text color", false, |n| Some(n.style.color.clone()), |n, v| {
                    if let Some(v) = v {
                        n.style.color = v;
                    }
                })}
                {length_field(store, id.clone(), "Padding", 96.0, |n| n.style.padding.clone(), |n, v| n.style.padding = v)}
            }
        });

        let (id_up, id_down, id_del) = (id.clone(), id.clone(), id.clone());
        let label = store.doc.with_untracked(|d| {
            find(store.nodes(d), &id)
                .map(tag_label)
                .unwrap_or(type_name)
        });
        view! {
            <div class="inspector">
                <h3>{label}</h3>
                {breakpoint.map(|bp| override_fields(store, id.clone(), bp))}
                {content}
                {binding_fields(store, id.clone())}
                {text_style}
                {(!is_instance).then(|| color_field(store, id.clone(), "Background", true, |n| n.style.background.clone(), |n, v| n.style.background = v))}
                {(!is_instance).then(|| {
                    let id = id.clone();
                    let make = move |_| {
                        let instance = new_id();
                        let mut made = false;
                        store.commit(|d| {
                            let tree = store.open_tree(d);
                            made = create_component(d, &tree, &id, new_id(), instance.clone()).is_some();
                        });
                        if made {
                            selected.set(Some(instance));
                        }
                    };
                    view! {
                        <button type="button" title="Reuse this block on any page; edits to it update everywhere" on:click=make>
                            "Make component"
                        </button>
                    }
                })}
                <div class="row actions">
                    <button type="button" title="Move up" on:click=move |_| store.commit(|d| { nudge(store.nodes_mut(d), &id_up, -1); })>
                        "↑"
                    </button>
                    <button type="button" title="Move down" on:click=move |_| store.commit(|d| { nudge(store.nodes_mut(d), &id_down, 1); })>
                        "↓"
                    </button>
                    <button
                        type="button"
                        class="danger"
                        on:click=move |_| {
                            store.commit(|d| {
                                remove(store.nodes_mut(d), &id_del);
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
