//! The Styles view: edit the document's design tokens (colors, type and spacing).

use leptos::prelude::*;
use web_sys::HtmlInputElement;

use vellum_core::{add_token, remove_token, rename_token};
use vellum_schema::{css, Document, TokenKind};

use crate::store::{use_store, Store};

#[component]
pub fn StylesView() -> impl IntoView {
    let store = use_store();
    // Rows are rebuilt only when a token is added or removed; their inputs read values live.
    let counts = Memo::new(move |_| {
        store.doc.with(|d| {
            (
                d.tokens.colors.len(),
                d.tokens.fonts.len(),
                d.tokens.spacing.len(),
            )
        })
    });
    let add = move |kind| {
        move |_| {
            store.commit(|d| {
                add_token(d, kind);
            })
        }
    };

    view! {
        <div class="styles-view">
            <section class="token-section">
                <header>
                    <h2>"Colors"</h2>
                    <p class="hint">"Each color has a light value and an optional dark one. Exports switch with the visitor's color scheme."</p>
                </header>
                <div class="token-head colors">
                    <span></span>
                    <span>"Name"</span>
                    <span>"Light"</span>
                    <span>"Dark"</span>
                    <span></span>
                </div>
                {move || (0..counts.get().0).map(|i| color_row(store, i)).collect_view()}
                <button type="button" on:click=add(TokenKind::Color)>"Add color"</button>
            </section>
            <section class="token-section">
                <header>
                    <h2>"Type"</h2>
                    <p class="hint">"Font family, size, weight and line height, used through a block's Type setting."</p>
                </header>
                {move || (0..counts.get().1).map(|i| font_row(store, i)).collect_view()}
                <button type="button" on:click=add(TokenKind::Font)>"Add type style"</button>
            </section>
            <section class="token-section">
                <header>
                    <h2>"Spacing"</h2>
                    <p class="hint">"Sizes for padding and gaps."</p>
                </header>
                {move || (0..counts.get().2).map(|i| space_row(store, i)).collect_view()}
                <button type="button" on:click=add(TokenKind::Space)>"Add spacing"</button>
            </section>
        </div>
    }
}

/// Reads one token value live, by kind and index.
fn read<T: Default>(store: Store, f: impl Fn(&Document) -> Option<T>) -> T {
    store.doc.with(f).unwrap_or_default()
}

fn name_of(d: &Document, kind: TokenKind, i: usize) -> Option<String> {
    d.tokens.names(kind).get(i).map(|n| (*n).to_owned())
}

/// The token's name, renamed (with every reference) when the input is committed.
fn name_input(store: Store, kind: TokenKind, i: usize) -> impl IntoView {
    let on_change = move |ev| {
        let input = event_target::<HtmlInputElement>(&ev);
        let Some(old) = store.doc.with_untracked(|d| name_of(d, kind, i)) else {
            return;
        };
        let mut renamed = false;
        store.commit(|d| renamed = rename_token(d, kind, &old, &input.value()));
        if !renamed {
            // Empty, unchanged or already taken: show the name that is still in effect.
            input.set_value(&old);
        } else if let Some(new) = store.doc.with_untracked(|d| name_of(d, kind, i)) {
            input.set_value(&new);
        }
    };
    let is_base = move || read(store, |d| Some(kind.is_base(&name_of(d, kind, i)?)));
    view! {
        <input
            class="token-name"
            aria-label="Token name"
            title=move || is_base().then_some("Page and button defaults use this color, so it keeps its name")
            prop:value=move || read(store, |d| name_of(d, kind, i))
            prop:disabled=is_base
            on:change=on_change
        />
    }
}

fn delete_button(store: Store, kind: TokenKind, i: usize) -> impl IntoView {
    let on_click = move |_| {
        if let Some(name) = store.doc.with_untracked(|d| name_of(d, kind, i)) {
            store.commit(|d| {
                remove_token(d, kind, &name);
            });
        }
    };
    let is_base = move || read(store, |d| Some(kind.is_base(&name_of(d, kind, i)?)));
    view! {
        <button
            type="button"
            class="ghost danger"
            class:invisible=is_base
            title="Delete; blocks keep its value"
            on:click=on_click
        >
            "×"
        </button>
    }
}

/// An input bound to one token value as text, with each edit applied live.
fn value_input(
    store: Store,
    attrs: (&'static str, &'static str),
    get: impl Fn(&Document) -> Option<String> + Send + Sync + 'static,
    set: impl Fn(&mut Document, String) + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let (input_type, label) = attrs;
    let on_input = move |ev| {
        let v = event_target_value(&ev);
        let set = set.clone();
        store.edit(move |d| set(d, v));
    };
    view! {
        <input
            type=input_type
            aria-label=label
            prop:value=move || read(store, &get)
            on:focus=move |_| store.begin_edit()
            on:input=on_input
        />
    }
}

fn color_row(store: Store, i: usize) -> impl IntoView {
    let swatch = move || {
        let dark = store.dark_preview.get();
        read(store, |d| {
            let t = d.tokens.colors.get(i)?;
            Some(match (&t.dark, dark) {
                (Some(v), true) => v.clone(),
                _ => t.light.clone(),
            })
        })
    };
    view! {
        <div class="token-row colors">
            <span class="swatch" style=move || format!("background:{}", swatch())></span>
            {name_input(store, TokenKind::Color, i)}
            {value_input(
                store,
                ("color", "Light value"),
                move |d| Some(d.tokens.colors.get(i)?.light.clone()),
                move |d, v| {
                    if let Some(t) = d.tokens.colors.get_mut(i) {
                        t.light = v;
                    }
                },
            )}
            {value_input(
                store,
                ("color", "Dark value"),
                move |d| {
                    let t = d.tokens.colors.get(i)?;
                    Some(t.dark.clone().unwrap_or_else(|| t.light.clone()))
                },
                move |d, v| {
                    if let Some(t) = d.tokens.colors.get_mut(i) {
                        t.dark = Some(v);
                    }
                },
            )}
            {delete_button(store, TokenKind::Color, i)}
        </div>
    }
}

fn font_row(store: Store, i: usize) -> impl IntoView {
    let preview = move || {
        read(store, |d| {
            let t = d.tokens.fonts.get(i)?;
            Some(format!(
                "font:{} {}px/{} {}",
                t.weight,
                css::number(t.size.min(40.0)),
                css::number(t.line_height),
                t.family
            ))
        })
    };
    view! {
        <div class="token-row fonts">
            {name_input(store, TokenKind::Font, i)}
            <p class="type-preview" style=preview>"The quick brown fox"</p>
            <div class="row">
                {value_input(
                    store,
                    ("text", "Font family"),
                    move |d| Some(d.tokens.fonts.get(i)?.family.clone()),
                    move |d, v| {
                        if let Some(t) = d.tokens.fonts.get_mut(i) {
                            t.family = v;
                        }
                    },
                )}
                {value_input(
                    store,
                    ("number", "Size in px"),
                    move |d| Some(d.tokens.fonts.get(i)?.size.to_string()),
                    move |d, v| {
                        if let (Some(t), Ok(v)) = (d.tokens.fonts.get_mut(i), v.parse()) {
                            t.size = v;
                        }
                    },
                )}
                {value_input(
                    store,
                    ("number", "Weight"),
                    move |d| Some(d.tokens.fonts.get(i)?.weight.to_string()),
                    move |d, v| {
                        if let (Some(t), Ok(v)) = (d.tokens.fonts.get_mut(i), v.parse()) {
                            t.weight = v;
                        }
                    },
                )}
                {value_input(
                    store,
                    ("number", "Line height"),
                    move |d| Some(d.tokens.fonts.get(i)?.line_height.to_string()),
                    move |d, v| {
                        if let (Some(t), Ok(v)) = (d.tokens.fonts.get_mut(i), v.parse()) {
                            t.line_height = v;
                        }
                    },
                )}
                {delete_button(store, TokenKind::Font, i)}
            </div>
        </div>
    }
}

fn space_row(store: Store, i: usize) -> impl IntoView {
    let bar = move || {
        read(store, |d| {
            Some(format!(
                "width:{}px",
                css::number(d.tokens.spacing.get(i)?.value)
            ))
        })
    };
    view! {
        <div class="token-row spacing">
            {name_input(store, TokenKind::Space, i)}
            {value_input(
                store,
                ("number", "Size in px"),
                move |d| Some(d.tokens.spacing.get(i)?.value.to_string()),
                move |d, v| {
                    if let (Some(t), Ok(v)) = (d.tokens.spacing.get_mut(i), v.parse()) {
                        t.value = v;
                    }
                },
            )}
            <span class="space-bar" style=bar></span>
            {delete_button(store, TokenKind::Space, i)}
        </div>
    }
}
