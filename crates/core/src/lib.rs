//! Editing operations on a [`Document`], kept free of any UI so they can be tested natively.

use vellum_schema::{Document, Drawing, Node, NodeKind, Style};

/// The kinds of block the page palette offers.
pub const PALETTE: &[(&str, &str)] = &[
    ("heading", "Heading"),
    ("text", "Text"),
    ("button", "Button"),
    ("image", "Image"),
    ("spacer", "Spacer"),
    ("svg", "Vector graphic"),
];

/// Inline placeholder so a fresh image block renders without network access.
pub const PLACEHOLDER_IMAGE: &str = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%22800%22%20height%3D%22400%22%3E%3Crect%20width%3D%22800%22%20height%3D%22400%22%20fill%3D%22%23eee9ff%22%2F%3E%3Cpath%20d%3D%22M260%20300l120-140%2090%20100%2060-60%20110%20100z%22%20fill%3D%22%23c7b8ff%22%2F%3E%3Ccircle%20cx%3D%22560%22%20cy%3D%22130%22%20r%3D%2240%22%20fill%3D%22%23c7b8ff%22%2F%3E%3C%2Fsvg%3E";

/// A new block of the given palette type with starter content, or `None` for an unknown type.
pub fn new_node(type_name: &str, id: String) -> Option<Node> {
    let kind = match type_name {
        "heading" => NodeKind::Heading {
            text: "A bold new heading".into(),
            level: 1,
        },
        "text" => NodeKind::Text {
            text: "Write something people will want to read.".into(),
        },
        "button" => NodeKind::Button {
            label: "Get started".into(),
            href: "#".into(),
        },
        "image" => NodeKind::Image {
            src: PLACEHOLDER_IMAGE.into(),
            alt: "Placeholder image".into(),
        },
        "spacer" => NodeKind::Spacer { height: 48.0 },
        "svg" => NodeKind::Svg {
            drawing: Drawing::default(),
        },
        _ => return None,
    };
    Some(Node {
        id,
        style: Style::default(),
        kind,
    })
}

/// The document a new project starts with.
pub fn starter_document(mut next_id: impl FnMut() -> String) -> Document {
    let mut doc = Document {
        title: "My site".into(),
        ..Document::default()
    };
    let mut heading = new_node("heading", next_id()).unwrap();
    heading.kind = NodeKind::Heading {
        text: "Welcome to Vellum".into(),
        level: 1,
    };
    let mut text = new_node("text", next_id()).unwrap();
    text.kind = NodeKind::Text {
        text: "Drag blocks from the left to build your page. Switch to Vector to draw graphics."
            .into(),
    };
    doc.nodes = vec![heading, text, new_node("button", next_id()).unwrap()];
    doc
}

pub fn index_of(doc: &Document, id: &str) -> Option<usize> {
    doc.nodes.iter().position(|n| n.id == id)
}

/// Inserts `node` at `at` (clamped to the end).
pub fn insert(doc: &mut Document, node: Node, at: usize) {
    let at = at.min(doc.nodes.len());
    doc.nodes.insert(at, node);
}

/// Moves the block at `from` so it lands at insertion index `to` (an index into the list
/// before removal, as a drop indicator reports it). Returns whether anything changed.
pub fn move_to(doc: &mut Document, from: usize, to: usize) -> bool {
    if from >= doc.nodes.len() || to > doc.nodes.len() || to == from || to == from + 1 {
        return false;
    }
    let node = doc.nodes.remove(from);
    doc.nodes.insert(if to > from { to - 1 } else { to }, node);
    true
}

/// Moves a block one step up (`-1`) or down (`1`).
pub fn nudge(doc: &mut Document, id: &str, delta: i32) -> bool {
    let Some(from) = index_of(doc, id) else {
        return false;
    };
    let to = if delta < 0 {
        from.checked_sub(1)
    } else {
        Some(from + 2)
    };
    match to {
        Some(to) => move_to(doc, from, to),
        None => false,
    }
}

/// Replaces the block with the same id. Returns whether it was found.
pub fn replace(doc: &mut Document, node: Node) -> bool {
    match doc.nodes.iter_mut().find(|n| n.id == node.id) {
        Some(slot) => {
            *slot = node;
            true
        }
        None => false,
    }
}

pub fn remove(doc: &mut Document, id: &str) -> Option<Node> {
    index_of(doc, id).map(|i| doc.nodes.remove(i))
}

/// Snapshot-based undo/redo. Documents are small, so whole copies keep this simple and exact.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Document>,
    redo: Vec<Document>,
}

const HISTORY_LIMIT: usize = 200;

impl History {
    /// Records `before` as the state to return to, and clears redo.
    pub fn record(&mut self, before: Document) {
        if self.undo.last() == Some(&before) {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: &Document) -> Option<Document> {
        let prev = self.undo.pop()?;
        self.redo.push(current.clone());
        Some(prev)
    }

    pub fn redo(&mut self, current: &Document) -> Option<Document> {
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(ids: &[&str]) -> Document {
        Document {
            nodes: ids
                .iter()
                .map(|id| new_node("text", id.to_string()).unwrap())
                .collect(),
            ..Document::default()
        }
    }

    fn ids(doc: &Document) -> Vec<&str> {
        doc.nodes.iter().map(|n| n.id.as_str()).collect()
    }

    #[test]
    fn move_down_to_insertion_index() {
        let mut d = doc(&["a", "b", "c"]);
        assert!(move_to(&mut d, 0, 3));
        assert_eq!(ids(&d), ["b", "c", "a"]);
    }

    #[test]
    fn move_up() {
        let mut d = doc(&["a", "b", "c"]);
        assert!(move_to(&mut d, 2, 0));
        assert_eq!(ids(&d), ["c", "a", "b"]);
    }

    #[test]
    fn drop_on_own_slot_is_a_no_op() {
        let mut d = doc(&["a", "b"]);
        assert!(!move_to(&mut d, 0, 1));
        assert!(!move_to(&mut d, 0, 0));
        assert_eq!(ids(&d), ["a", "b"]);
    }

    #[test]
    fn nudge_stops_at_the_ends() {
        let mut d = doc(&["a", "b", "c"]);
        assert!(!nudge(&mut d, "a", -1));
        assert!(nudge(&mut d, "a", 1));
        assert_eq!(ids(&d), ["b", "a", "c"]);
        assert!(nudge(&mut d, "c", -1));
        assert_eq!(ids(&d), ["b", "c", "a"]);
        assert!(!nudge(&mut d, "a", 1));
    }

    #[test]
    fn insert_clamps_and_remove_returns_node() {
        let mut d = doc(&["a"]);
        insert(&mut d, new_node("spacer", "s".into()).unwrap(), 99);
        assert_eq!(ids(&d), ["a", "s"]);
        assert_eq!(remove(&mut d, "a").map(|n| n.id), Some("a".into()));
        assert_eq!(ids(&d), ["s"]);
    }

    #[test]
    fn undo_and_redo_walk_history() {
        let mut h = History::default();
        let mut d = doc(&["a"]);
        h.record(d.clone());
        insert(&mut d, new_node("text", "b".into()).unwrap(), 1);

        let undone = h.undo(&d).unwrap();
        assert_eq!(ids(&undone), ["a"]);
        let redone = h.redo(&undone).unwrap();
        assert_eq!(ids(&redone), ["a", "b"]);
        assert!(h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn unknown_palette_type_is_rejected() {
        assert!(new_node("carousel", "x".into()).is_none());
        for (t, _) in PALETTE {
            assert_eq!(new_node(t, "x".into()).unwrap().kind.type_name(), *t);
        }
    }
}
