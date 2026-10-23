//! Site-level controls in the page editor: the pages list, the breakpoint switcher and the
//! inspector section that edits a block's breakpoint overrides.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use vellum_core::{add_page, find, find_mut, nudge_page, remove_page, set_page_path};
use vellum_schema::{Align, Breakpoint, Direction, NodeId, NodeKind, Override, TokenKind};

use crate::store::{new_id, Store};

/// Lists the site's pages, opens one, and edits the open page's name and path.
#[component]
pub fn PagesPanel(selected: RwSignal<Option<NodeId>>) -> impl IntoView {
    let store = expect_context::<Store>();
    let open = move |id: NodeId| {
        selected.set(None);
        store.page.set(id);
    };
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
                            class:active=move || current.with(|c| *c == id_active)
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

    view! {
        <h2>"Pages"</h2>
        {list}
        <button type="button" on:click=add>"+ Add page"</button>
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
        <hr />
    }
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
        <div class="breakpoints tabs">
            {option(None, "Desktop", "Base styles, used at every width".into())}
            {options}
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
