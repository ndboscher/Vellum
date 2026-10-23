//! Site-level controls in the page editor: the pages list, the breakpoint switcher and the
//! inspector section that edits a block's breakpoint overrides.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use vellum_core::{
    add_page, add_prop, bind, detach, find, find_mut, instance_count, nudge_page, remove_component,
    remove_page, remove_prop, rename_prop, set_page_path,
};
use vellum_schema::{
    Align, Breakpoint, Direction, NodeId, NodeKind, Override, PropKind, TokenKind,
};

use crate::store::{new_id, Store};

/// Lists the site's pages, opens one, and edits the open page's name and path.
#[component]
pub fn PagesPanel(selected: RwSignal<Option<NodeId>>) -> impl IntoView {
    let store = expect_context::<Store>();
    let open = move |id: NodeId| {
        selected.set(None);
        store.open(id);
    };
    let editing_component = move || store.open_component().is_some();
    // The page actually shown, which falls back to home if the open one is gone.
    let current = Memo::new(move |_| {
        let id = store.page.get();
        store.doc.with(|d| d.page_or_home(&id).id.clone())
    });

    let list = move || {
        store.doc.with(|d| {
            d.pages
                .iter()
                .map(|p| {
                    let id = p.id.clone();
                    let id_active = id.clone();
                    view! {
                        <button
                            type="button"
                            class="palette-item page-item"
                            class:active=move || !editing_component() && current.with(|c| *c == id_active)
                            on:click=move |_| open(id.clone())
                        >
                            <span>{p.name.clone()}</span>
                            <kbd>{p.path.clone()}</kbd>
                        </button>
                    }
                })
                .collect_view()
        })
    };

    let add = move |_| {
        let mut id = None;
        store.commit(|d| id = Some(add_page(d, new_id())));
        if let Some(id) = id {
            open(id);
        }
    };

    let page_value = move |f: fn(&vellum_schema::Page) -> String| {
        let id = current.get();
        store.doc.with(|d| d.page(&id).map(f).unwrap_or_default())
    };
    let is_home = move || page_value(|p| p.path.clone()) == "/";
    let on_name = move |ev| {
        let name = event_target_value(&ev);
        let id = current.get_untracked();
        store.edit(|d| {
            if let Some(p) = d.page_mut(&id) {
                p.name = name;
            }
        });
    };
    let on_path = move |ev: leptos::ev::Event| {
        let input = event_target_value(&ev);
        let id = current.get_untracked();
        let mut changed = false;
        store.commit(|d| changed = set_page_path(d, &id, &input));
        if !changed {
            // Refused (taken, empty or reserved): show the path that stands.
            let path = store
                .doc
                .with_untracked(|d| d.page(&id).map(|p| p.path.clone()));
            if let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            {
                el.set_value(&path.unwrap_or_default());
            }
        }
    };
    let nudge = move |delta| {
        let id = current.get_untracked();
        store.commit(|d| {
            nudge_page(d, &id, delta);
        });
    };
    let delete = move |_| {
        let id = current.get_untracked();
        store.commit(|d| {
            remove_page(d, &id);
        });
        open("home".into());
    };

    let component_list = component_list_view(store, selected);

    view! {
        <h2>"Pages"</h2>
        {list}
        <button type="button" on:click=add>"+ Add page"</button>
        <div class="fields" class:hidden=editing_component>
        <label class="field">
            <span>"Page name"</span>
            <input prop:value=move || page_value(|p| p.name.clone()) on:focus=move |_| store.begin_edit() on:input=on_name />
        </label>
        <label class="field">
            <span>"Path"</span>
            <input
                prop:value=move || page_value(|p| p.path.clone())
                disabled=is_home
                title=move || is_home().then_some("The home page is always at /")
                on:change=on_path
            />
        </label>
        <div class="row" class:hidden=is_home>
            <button type="button" title="Earlier in navigation" on:click=move |_| nudge(-1)>"↑"</button>
            <button type="button" title="Later in navigation" on:click=move |_| nudge(1)>"↓"</button>
            <button type="button" class="danger" on:click=delete>"Delete page"</button>
        </div>
        </div>
        <hr />
        <h2>"Components"</h2>
        {component_list}
        {move || store.open_component().map(|id| component_settings(store, id))}
        <hr />
    }
}

/// The component list in the pages panel: click one to edit it.
fn component_list_view(store: Store, selected: RwSignal<Option<NodeId>>) -> impl IntoView {
    move || {
        let open = store.page.get();
        store.doc.with(|d| {
            if d.components.is_empty() {
                return view! {
                    <p class="hint">"Select a block and choose Make component to reuse it."</p>
                }
                .into_any();
            }
            d.components
                .iter()
                .map(|comp| {
                    let id = comp.id.clone();
                    let active = id == open;
                    view! {
                        <button
                            type="button"
                            class="palette-item page-item"
                            class:active=active
                            on:click=move |_| {
                                selected.set(None);
                                store.open(id.clone());
                            }
                        >
                            <span>{comp.name.clone()}</span>
                            <kbd>{instance_count(d, &comp.id).to_string()}" used"</kbd>
                        </button>
                    }
                })
                .collect_view()
                .into_any()
        })
    }
}

/// Name, props and deletion of the open component.
fn component_settings(store: Store, id: NodeId) -> AnyView {
    // Rows are keyed on the prop list's shape so typing a default doesn't rebuild the input.
    let id_props = id.clone();
    let props = Memo::new(move |_| {
        store.doc.with(|d| {
            d.component(&id_props)
                .map(|c| {
                    c.props
                        .iter()
                        .map(|p| (p.name.clone(), p.kind))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    let id_name = id.clone();
    let id_on_name = id.clone();
    let name = move || {
        store.doc.with(|d| {
            d.component(&id_name)
                .map(|c| c.name.clone())
                .unwrap_or_default()
        })
    };
    let on_name = move |ev| {
        let v = event_target_value(&ev);
        store.edit(|d| {
            if let Some(c) = d.component_mut(&id_on_name) {
                c.name = v;
            }
        });
    };
    let id_rows = id.clone();
    let rows = move || {
        let id = id_rows.clone();
        props
            .get()
            .into_iter()
            .map(|(prop, kind)| prop_row(store, id.clone(), prop, kind))
            .collect_view()
    };
    let add = |kind: PropKind| {
        let id = id.clone();
        move |_| {
            store.commit(|d| {
                add_prop(d, &id, kind);
            });
        }
    };
    let id_delete = id.clone();
    let delete = move |_| {
        store.commit(|d| {
            remove_component(d, &id_delete, &mut new_id);
        });
        store.page.set(store.back_to.get_untracked());
    };
    view! {
        <div class="fields component-settings">
            <label class="field">
                <span>"Component name"</span>
                <input prop:value=name on:focus=move |_| store.begin_edit() on:input=on_name />
            </label>
            <span class="hint">"Props: values each placement can set. Bind a block's text or link to one in the inspector."</span>
            {rows}
            <div class="row">
                <button type="button" on:click=add(PropKind::Text)>"+ Text"</button>
                <button type="button" on:click=add(PropKind::Link)>"+ Link"</button>
                <button type="button" on:click=add(PropKind::Image)>"+ Image"</button>
            </div>
            <button type="button" class="danger" title="Placements keep their content as plain blocks" on:click=delete>
                "Delete component"
            </button>
        </div>
    }
    .into_any()
}

/// One prop of a component: its name, kind and default.
fn prop_row(store: Store, component: NodeId, prop: String, kind: PropKind) -> impl IntoView {
    let (c1, c2, c3, c4) = (
        component.clone(),
        component.clone(),
        component.clone(),
        component,
    );
    let (p1, p2, p3, p4) = (prop.clone(), prop.clone(), prop.clone(), prop.clone());
    let default = move || {
        store.doc.with(|d| {
            d.component(&c1)
                .and_then(|c| c.prop(&p1))
                .map(|p| p.default.clone())
                .unwrap_or_default()
        })
    };
    let on_default = move |ev| {
        let v = event_target_value(&ev);
        store.edit(|d| {
            if let Some(p) = d
                .component_mut(&c2)
                .and_then(|c| c.props.iter_mut().find(|p| p.name == p2))
            {
                p.default = v;
            }
        });
    };
    let on_rename = move |ev: leptos::ev::Event| {
        let v = event_target_value(&ev);
        let mut renamed = false;
        store.commit(|d| renamed = rename_prop(d, &c3, &p3, &v));
        if !renamed {
            reset_input(&ev, &p3);
        }
    };
    let remove = move |_| {
        store.commit(|d| {
            remove_prop(d, &c4, &p4);
        });
    };
    view! {
        <div class="prop-row">
            <div class="row">
                <input class="prop-name" prop:value=prop.clone() title="Prop name" on:change=on_rename />
                <kbd>{kind.as_str()}</kbd>
                <button type="button" class="ghost" title="Remove prop" on:click=remove>"✕"</button>
            </div>
            <input prop:value=default placeholder="Default value" on:focus=move |_| store.begin_edit() on:input=on_default />
        </div>
    }
}

/// Puts an input's text back to `value` after a refused change.
fn reset_input(ev: &leptos::ev::Event, value: &str) {
    if let Some(el) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
    {
        el.set_value(value);
    }
}

/// Shown above the canvas while a component is open.
#[component]
pub fn ComponentBanner(selected: RwSignal<Option<NodeId>>) -> impl IntoView {
    let store = expect_context::<Store>();
    move || {
        let id = store.open_component()?;
        let (name, uses) = store.doc.with(|d| {
            let name = d.component(&id).map(|c| c.name.clone()).unwrap_or_default();
            (name, instance_count(d, &id))
        });
        let done = move |ev: leptos::ev::MouseEvent| {
            ev.stop_propagation();
            selected.set(None);
            store.page.set(store.back_to.get_untracked());
        };
        Some(view! {
            <div class="component-banner">
                <span>
                    "Editing component " <strong>{name}</strong>
                    {format!(" · placed {uses} time{}", if uses == 1 { "" } else { "s" })}
                </span>
                <button type="button" class="primary" on:click=done>"Done"</button>
            </div>
        })
    }
}

/// The inspector fields of a component instance: a value for each prop.
pub fn instance_fields(store: Store, id: NodeId, selected: RwSignal<Option<NodeId>>) -> AnyView {
    let id_memo = id.clone();
    let shape = Memo::new(move |_| {
        store.doc.with(|d| {
            let n = find(store.nodes(d), &id_memo)?;
            let NodeKind::Instance { component, .. } = &n.kind else {
                return None;
            };
            let c = d.component(component)?;
            let props: Vec<_> = c
                .props
                .iter()
                .map(|p| (p.name.clone(), p.kind, p.default.clone()))
                .collect();
            Some((c.id.clone(), c.name.clone(), props))
        })
    });
    let id_fields = id.clone();
    let fields = move || {
        let (_, _, props) = shape.get()?;
        let rows = props
            .into_iter()
            .map(|(prop, kind, default)| {
                let (id_read, id_write) = (id_fields.clone(), id_fields.clone());
                let (p_read, p_write) = (prop.clone(), prop.clone());
                let value = move || {
                    store.doc.with(|d| {
                        find(store.nodes(d), &id_read)
                            .and_then(|n| match &n.kind {
                                NodeKind::Instance { props, .. } => props.get(&p_read).cloned(),
                                _ => None,
                            })
                            .unwrap_or_default()
                    })
                };
                let on_input = move |ev| {
                    let v = event_target_value(&ev);
                    store.edit(|d| {
                        if let Some(NodeKind::Instance { props, .. }) =
                            find_mut(store.nodes_mut(d), &id_write).map(|n| &mut n.kind)
                        {
                            if v.is_empty() {
                                props.remove(&p_write);
                            } else {
                                props.insert(p_write.clone(), v);
                            }
                        }
                    });
                };
                view! {
                    <label class="field">
                        <span>{format!("{prop} ({})", kind.as_str())}</span>
                        <input prop:value=value placeholder=default on:focus=move |_| store.begin_edit() on:input=on_input />
                    </label>
                }
            })
            .collect_view();
        Some(rows)
    };
    let edit_component = move |_| {
        if let Some((component, _, _)) = shape.get_untracked() {
            selected.set(None);
            store.open(component);
        }
    };
    let id_detach = id.clone();
    let detach_instance = move |_| {
        store.commit(|d| {
            let tree = store.open_tree(d);
            detach(d, &tree, &id_detach, &mut new_id);
        });
        selected.set(None);
    };
    view! {
        <p class="hint">
            {move || shape.get().map(|(_, name, props)| {
                if props.is_empty() {
                    format!("A placement of {name}. Give it props to vary its content here.")
                } else {
                    format!("A placement of {name}. Empty fields use the default.")
                }
            })}
        </p>
        {fields}
        <div class="row">
            <button type="button" class="primary" on:click=edit_component>"Edit component"</button>
            <button type="button" title="Turn this placement back into plain blocks" on:click=detach_instance>"Detach"</button>
        </div>
    }
    .into_any()
}

/// While editing a component: which of the selected block's fields show a prop.
pub fn binding_fields(store: Store, id: NodeId) -> AnyView {
    let view = move || {
        let component = store.open_component()?;
        let (fields, props) = store.doc.with(|d| {
            let n = find(store.nodes(d), &id)?;
            let props: Vec<_> = d
                .component(&component)?
                .props
                .iter()
                .map(|p| (p.name.clone(), p.kind))
                .collect();
            Some((n.kind.bindable_fields(), props))
        })?;
        if fields.is_empty() {
            return None;
        }
        let rows = fields
            .iter()
            .map(|&(field, kind)| {
                let (id_read, id_write) = (id.clone(), id.clone());
                let component = component.clone();
                let value = move || {
                    store.doc.with(|d| {
                        find(store.nodes(d), &id_read)
                            .and_then(|n| n.bindings.get(field).cloned())
                            .unwrap_or_default()
                    })
                };
                let on_change = move |ev| {
                    let v = event_target_value(&ev);
                    store.commit(|d| {
                        bind(
                            d,
                            &component,
                            &id_write,
                            field,
                            (!v.is_empty()).then_some(v.as_str()),
                        );
                    });
                };
                let options = props
                    .iter()
                    .map(|(name, prop_kind)| {
                        let label = if *prop_kind == kind {
                            name.clone()
                        } else {
                            format!("{name} ({})", prop_kind.as_str())
                        };
                        view! { <option value=name.clone()>{label}</option> }
                    })
                    .collect_view();
                view! {
                    <label class="field">
                        <span>{format!("{} shows", capitalize(field))}</span>
                        <select prop:value=value on:change=on_change>
                            <option value="">"Its own value"</option>
                            {options}
                        </select>
                    </label>
                }
            })
            .collect_view();
        Some(view! {
            <div class="overrides bindings">
                <h3>"Props"</h3>
                {props.is_empty().then(|| view! {
                    <p class="hint">"Add props to this component in the left panel, then bind fields to them here."</p>
                })}
                {rows}
            </div>
        })
    };
    view.into_any()
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Desktop, tablet and mobile previews of the canvas.
#[component]
pub fn BreakpointBar() -> impl IntoView {
    let store = expect_context::<Store>();
    let option = move |bp: Option<Breakpoint>, label: &'static str, hint: String| {
        view! {
            <button
                type="button"
                title=hint
                class:active=move || store.breakpoint.get() == bp
                on:click=move |ev| {
                    ev.stop_propagation();
                    store.breakpoint.set(bp);
                }
            >
                {label}
            </button>
        }
    };
    let options = Breakpoint::ALL
        .into_iter()
        .map(|bp| {
            option(
                Some(bp),
                bp.label(),
                format!(
                    "Preview and edit styles at {}px and narrower",
                    bp.max_width()
                ),
            )
        })
        .collect_view();
    view! {
        <div class="row">
            <div class="breakpoints tabs">
                {option(None, "Desktop", "Base styles, used at every width".into())}
                {options}
            </div>
            <button
                type="button"
                class="ghost"
                title="Play the page's animations again"
                on:click=move |ev| {
                    ev.stop_propagation();
                    store.animation_generation.update(|g| *g += 1);
                }
            >
                "▶ Replay"
            </button>
        </div>
    }
}

/// How wide the canvas page is when previewing `bp`, as an inline style.
pub fn preview_width(bp: Option<Breakpoint>) -> &'static str {
    match bp {
        None => "",
        Some(Breakpoint::Tablet) => ";max-width:768px",
        Some(Breakpoint::Mobile) => ";max-width:390px",
    }
}

const INHERIT: &str = "";

/// Reads one override field of node `id` at `bp`, live.
fn read_override<T: Default>(
    store: Store,
    id: &str,
    bp: Breakpoint,
    f: impl Fn(&Override) -> T,
) -> T {
    store.doc.with(|d| {
        find(store.nodes(d), id)
            .and_then(|n| n.responsive.get(&bp))
            .map(f)
            .unwrap_or_default()
    })
}

/// Changes the override of node `id` at `bp`, dropping it once nothing is left in it.
fn commit_override(store: Store, id: &str, bp: Breakpoint, f: impl FnOnce(&mut Override)) {
    store.commit(|d| {
        if let Some(n) = find_mut(store.nodes_mut(d), id) {
            let o = n.responsive.entry(bp).or_default();
            f(o);
            if o.is_empty() {
                n.responsive.remove(&bp);
            }
        }
    });
}

/// A select over one override field, with "Inherit" (unset) first.
fn override_select(
    store: Store,
    id: NodeId,
    bp: Breakpoint,
    label: &'static str,
    options: Vec<(String, String)>,
    get: fn(&Override) -> String,
    set: fn(&mut Override, &str),
) -> AnyView {
    let id_read = id.clone();
    let value = move || read_override(store, &id_read, bp, get);
    let on_change = move |ev| {
        let v = event_target_value(&ev);
        commit_override(store, &id, bp, |o| set(o, &v));
    };
    view! {
        <label class="field">
            <span>{label}</span>
            <select prop:value=value on:change=on_change>
                <option value=INHERIT>"Inherit"</option>
                {options.into_iter().map(|(v, l)| view! { <option value=v>{l}</option> }).collect_view()}
            </select>
        </label>
    }
    .into_any()
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(v, l)| ((*v).to_owned(), (*l).to_owned()))
        .collect()
}

/// The inspector section for the selected block's overrides at the previewed breakpoint.
pub fn override_fields(store: Store, id: NodeId, bp: Breakpoint) -> AnyView {
    let (kind, tokens) = store.doc.with_untracked(|d| {
        let kind = find(store.nodes(d), &id).map(|n| n.kind.clone());
        let names = |k| {
            d.tokens
                .names(k)
                .into_iter()
                .map(|n| (n.to_owned(), n.to_owned()))
                .collect::<Vec<_>>()
        };
        (kind, (names(TokenKind::Space), names(TokenKind::Font)))
    });
    let Some(kind) = kind else {
        return ().into_any();
    };
    let (space, fonts) = tokens;

    let id_hidden = id.clone();
    let id_toggle = id.clone();
    let hidden = move || read_override(store, &id_hidden, bp, |o| o.hidden);
    let toggle = move |ev: leptos::ev::Event| {
        let on = event_target_checked(&ev);
        commit_override(store, &id_toggle, bp, |o| o.hidden = on);
    };
    let id_clear = id.clone();
    let has_overrides = {
        let id = id.clone();
        move || read_override(store, &id, bp, |o| !o.is_empty())
    };
    let clear = move |_| {
        store.commit(|d| {
            if let Some(n) = find_mut(store.nodes_mut(d), &id_clear) {
                n.responsive.remove(&bp);
            }
        });
    };

    let sel =
        |label, options, get, set| override_select(store, id.clone(), bp, label, options, get, set);
    let is_spacer = matches!(kind, NodeKind::Spacer { .. });
    let style_fields = (!is_spacer).then(|| {
        view! {
            {sel(
                "Align",
                pairs(&[("left", "Left"), ("center", "Center"), ("right", "Right")]),
                |o| o.align.map(|a| a.as_css().to_owned()).unwrap_or_default(),
                |o, v| {
                    o.align = match v {
                        "left" => Some(Align::Left),
                        "center" => Some(Align::Center),
                        "right" => Some(Align::Right),
                        _ => None,
                    }
                },
            )}
            {sel(
                "Padding",
                space.clone(),
                |o| match &o.padding {
                    Some(vellum_schema::Length::Token(t)) => t.clone(),
                    _ => String::new(),
                },
                |o, v| o.padding = (!v.is_empty()).then(|| vellum_schema::Length::Token(v.to_owned())),
            )}
            {sel(
                "Type",
                fonts,
                |o| o.font.clone().unwrap_or_default(),
                |o, v| o.font = (!v.is_empty()).then(|| v.to_owned()),
            )}
        }
    });
    let layout_fields = match &kind {
        NodeKind::Container { layout, .. } => {
            let mode = if layout.mode == vellum_schema::LayoutMode::Grid {
                sel(
                    "Columns",
                    (1..=6).map(|n| (n.to_string(), n.to_string())).collect(),
                    |o| o.columns.map(|c| c.to_string()).unwrap_or_default(),
                    |o, v| o.columns = v.parse().ok(),
                )
            } else {
                sel(
                    "Direction",
                    pairs(&[("row", "Row"), ("column", "Column")]),
                    |o| {
                        o.direction
                            .map(|d| d.as_str().to_owned())
                            .unwrap_or_default()
                    },
                    |o, v| o.direction = Direction::parse(v),
                )
            };
            Some(view! {
                {mode}
                {sel(
                    "Gap",
                    space,
                    |o| match &o.gap {
                        Some(vellum_schema::Length::Token(t)) => t.clone(),
                        _ => String::new(),
                    },
                    |o, v| o.gap = (!v.is_empty()).then(|| vellum_schema::Length::Token(v.to_owned())),
                )}
            })
        }
        _ => None,
    };

    view! {
        <div class="overrides">
            <h3>{format!("{} overrides", bp.label())}</h3>
            <p class="hint">
                {format!("These apply at {}px and narrower. The fields below them set the base style.", bp.max_width())}
            </p>
            <label class="check">
                <input type="checkbox" prop:checked=hidden on:change=toggle />
                {format!("Hide on {}", bp.label().to_lowercase())}
            </label>
            {style_fields}
            {layout_fields}
            <button type="button" disabled=move || !has_overrides() on:click=clear>
                "Clear overrides"
            </button>
        </div>
    }
    .into_any()
}
