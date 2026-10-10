//! The editor shell: top bar, view switching, persistence and global shortcuts.

use leptos::ev;
use leptos::prelude::*;

use vellum_core::starter_document;

use crate::page::PageEditor;
use crate::persistence;
use crate::store::{is_typing, new_id, Store, View};
use crate::vector::VectorCanvas;

#[component]
pub fn App() -> impl IntoView {
    let store = Store::new(persistence::load_drawing());
    provide_context(store);
    persistence::load(store, || starter_document(new_id));

    Effect::new(move |_| {
        let doc = store.doc.get();
        if store.loaded.get_untracked() {
            persistence::schedule_save(store, doc);
        }
    });
    Effect::new(move |_| store.drawing.with(persistence::save_drawing));

    let keys = window_event_listener(ev::keydown, move |ev| {
        if is_typing(&ev)
            || store.view.get_untracked() != View::Page
            || !(ev.ctrl_key() || ev.meta_key())
        {
            return;
        }
        match ev.key().to_lowercase().as_str() {
            "z" if ev.shift_key() => store.redo(),
            "z" => store.undo(),
            "y" => store.redo(),
            _ => return,
        }
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());

    let tab = move |v: View, label: &'static str| {
        view! {
            <button type="button" class:active=move || store.view.get() == v on:click=move |_| store.view.set(v)>
                {label}
            </button>
        }
    };

    view! {
        <div class="app">
            <header class="topbar">
                <span class="brand">"Vellum"</span>
                <nav class="tabs">{tab(View::Page, "Page")} {tab(View::Vector, "Vector")}</nav>
                <div class="row">
                    <button
                        type="button"
                        title="Undo (Ctrl+Z)"
                        disabled=move || !store.can_undo.get()
                        on:click=move |_| store.undo()
                    >
                        "Undo"
                    </button>
                    <button
                        type="button"
                        title="Redo (Ctrl+Shift+Z)"
                        disabled=move || !store.can_redo.get()
                        on:click=move |_| store.redo()
                    >
                        "Redo"
                    </button>
                </div>
                <span class="status">{move || store.save_status.get()}</span>
            </header>
            {move || match store.view.get() {
                View::Page => view! { <PageEditor /> }.into_any(),
                View::Vector => view! { <VectorCanvas /> }.into_any(),
            }}
        </div>
    }
}
