//! Editor state shared by every panel, provided through Leptos context.

use leptos::prelude::*;
use vellum_core::History;
use vellum_schema::{Breakpoint, Document, Drawing, Node, NodeId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Page,
    Vector,
    Styles,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub doc: RwSignal<Document>,
    history: StoredValue<History>,
    /// Snapshot taken when an input gains focus, recorded on its first edit so a typing
    /// session is one undo step.
    pending: StoredValue<Option<Document>>,
    pub can_undo: RwSignal<bool>,
    pub can_redo: RwSignal<bool>,
    /// False until the document has been loaded, so the empty placeholder is never saved.
    pub loaded: RwSignal<bool>,
    pub save_status: RwSignal<&'static str>,
    pub view: RwSignal<View>,
    /// Show the page with the dark values of color tokens.
    pub dark_preview: RwSignal<bool>,
    /// The drawing open in the vector canvas.
    pub drawing: RwSignal<Drawing>,
    /// The page block the open drawing belongs to, if it was opened from one.
    pub editing_node: RwSignal<Option<NodeId>>,
    /// The page or component open in the page editor. If it disappears (say, on undo) the
    /// home page shows.
    pub page: RwSignal<NodeId>,
    /// The page to go back to when done editing a component.
    pub back_to: RwSignal<NodeId>,
    /// The viewport the canvas previews, `None` for desktop. Responsive controls edit its
    /// overrides.
    pub breakpoint: RwSignal<Option<Breakpoint>>,
}

impl Store {
    pub fn new(drawing: Drawing) -> Self {
        Self {
            doc: RwSignal::new(Document::default()),
            history: StoredValue::new(History::default()),
            pending: StoredValue::new(None),
            can_undo: RwSignal::new(false),
            can_redo: RwSignal::new(false),
            loaded: RwSignal::new(false),
            save_status: RwSignal::new("Loading…"),
            view: RwSignal::new(View::Page),
            dark_preview: RwSignal::new(false),
            drawing: RwSignal::new(drawing),
            editing_node: RwSignal::new(None),
            page: RwSignal::new("home".into()),
            back_to: RwSignal::new("home".into()),
            breakpoint: RwSignal::new(None),
        }
    }

    /// The open page's or component's top level. Untracked: views that list it also track
    /// `page`.
    pub fn nodes<'a>(&self, d: &'a Document) -> &'a Vec<Node> {
        self.page
            .with_untracked(|id| d.tree(id).unwrap_or(&d.pages[0].nodes))
    }

    pub fn nodes_mut<'a>(&self, d: &'a mut Document) -> &'a mut Vec<Node> {
        let id = self.page.get_untracked();
        if d.tree(&id).is_some() {
            d.tree_mut(&id).expect("checked above")
        } else {
            &mut d.pages[0].nodes
        }
    }

    /// The id of the open page or component, or home's if it is gone.
    pub fn open_tree(&self, d: &Document) -> NodeId {
        let id = self.page.get_untracked();
        if d.tree(&id).is_some() {
            id
        } else {
            d.pages[0].id.clone()
        }
    }

    /// The component open for editing, if one is. Tracks `page` and the document.
    pub fn open_component(&self) -> Option<NodeId> {
        let id = self.page.get();
        self.doc.with(|d| d.component(&id).map(|c| c.id.clone()))
    }

    /// Opens a page or component, remembering the last page to come back to.
    pub fn open(&self, id: NodeId) {
        if self.doc.with_untracked(|d| d.page(&id).is_some()) {
            self.back_to.set(id.clone());
        }
        self.page.set(id);
    }

    /// Finds a node in any page or component, so a drawing opened from one still saves back.
    pub fn find_mut<'a>(d: &'a mut Document, id: &str) -> Option<&'a mut Node> {
        d.trees_mut()
            .find_map(|nodes| vellum_core::find_mut(nodes, id))
    }

    /// Applies a discrete change (insert, move, delete) as its own undo step.
    pub fn commit(&self, f: impl FnOnce(&mut Document)) {
        self.pending.set_value(None);
        let before = self.doc.get_untracked();
        self.doc.update(f);
        if self.doc.with_untracked(|d| *d != before) {
            self.history.update_value(|h| h.record(before));
            self.sync_flags();
        }
    }

    /// Marks the start of a continuous edit (typing, dragging a slider).
    pub fn begin_edit(&self) {
        self.pending.set_value(Some(self.doc.get_untracked()));
    }

    /// Applies one step of a continuous edit; the first step records the undo point.
    pub fn edit(&self, f: impl FnOnce(&mut Document)) {
        let mut snapshot = None;
        self.pending.update_value(|p| snapshot = p.take());
        if let Some(before) = snapshot {
            self.history.update_value(|h| h.record(before));
            self.sync_flags();
        }
        self.doc.update(f);
    }

    pub fn undo(&self) {
        let current = self.doc.get_untracked();
        let mut prev = None;
        self.history.update_value(|h| prev = h.undo(&current));
        if let Some(prev) = prev {
            self.doc.set(prev);
            self.sync_flags();
        }
    }

    pub fn redo(&self) {
        let current = self.doc.get_untracked();
        let mut next = None;
        self.history.update_value(|h| next = h.redo(&current));
        if let Some(next) = next {
            self.doc.set(next);
            self.sync_flags();
        }
    }

    fn sync_flags(&self) {
        self.history.with_value(|h| {
            self.can_undo.set(h.can_undo());
            self.can_redo.set(h.can_redo());
        });
    }
}

pub fn use_store() -> Store {
    expect_context::<Store>()
}

/// A short random id for nodes and shapes.
pub fn new_id() -> String {
    format!("{:x}", (js_sys::Math::random() * 2f64.powi(48)) as u64)
}

/// True when a key event comes from a text field, so global shortcuts leave it alone.
pub fn is_typing(ev: &web_sys::KeyboardEvent) -> bool {
    use wasm_bindgen::JsCast;
    ev.target().is_some_and(|t| {
        t.dyn_ref::<web_sys::HtmlInputElement>().is_some()
            || t.dyn_ref::<web_sys::HtmlTextAreaElement>().is_some()
    })
}
