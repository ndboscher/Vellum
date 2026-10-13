//! Editing operations on a [`Document`], kept free of any UI so they can be tested natively.

use vellum_schema::{
    css, parse_token_ref, token_ref, ColorToken, Document, Drawing, FontToken, Layout, Length,
    Node, NodeId, NodeKind, SpaceToken, Style, TokenKind,
};

/// The kinds of block the page palette offers. Containers come in three starting layouts.
pub const PALETTE: &[(&str, &str)] = &[
    ("stack", "Stack"),
    ("row", "Row"),
    ("grid", "Grid"),
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
    let mut style = Style::default();
    let kind = match type_name {
        "stack" | "row" | "grid" => {
            let layout = match type_name {
                "stack" => Layout::stack(),
                "row" => Layout::row(),
                _ => Layout::grid(3),
            };
            NodeKind::Container {
                layout,
                children: Vec::new(),
            }
        }
        "heading" => NodeKind::Heading {
            text: "A bold new heading".into(),
            level: 1,
        },
        "text" => {
            style.font = Some("body".into());
            NodeKind::Text {
                text: "Write something people will want to read.".into(),
            }
        }
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
    Some(Node { id, style, kind })
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
    heading.style.font = Some("display".into());
    let mut text = new_node("text", next_id()).unwrap();
    text.kind = NodeKind::Text {
        text: "Drag blocks from the left to build your page. Drop them into a Row or Grid to lay them out side by side, and edit colors and type under Styles."
            .into(),
    };
    let mut row = new_node("row", next_id()).unwrap();
    let mut secondary = new_node("button", next_id()).unwrap();
    secondary.kind = NodeKind::Button {
        label: "See examples".into(),
        href: "#".into(),
    };
    if let Some(children) = row.children_mut() {
        children.extend([new_node("button", next_id()).unwrap(), secondary]);
    }
    doc.nodes = vec![heading, text, row];
    doc
}

/// The node with this id anywhere in the tree.
pub fn find<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|n| {
        if n.id == id {
            Some(n)
        } else {
            find(n.children()?, id)
        }
    })
}

pub fn find_mut<'a>(nodes: &'a mut [Node], id: &str) -> Option<&'a mut Node> {
    for n in nodes {
        if n.id == id {
            return Some(n);
        }
        if let Some(found) = n.children_mut().and_then(|c| find_mut(c, id)) {
            return Some(found);
        }
    }
    None
}

/// The child list of `parent`, or the page's top level for `None`. `None` if `parent` is
/// missing or not a container.
pub fn children<'a>(doc: &'a Document, parent: Option<&str>) -> Option<&'a Vec<Node>> {
    match parent {
        None => Some(&doc.nodes),
        Some(id) => find(&doc.nodes, id)?.children(),
    }
}

pub fn children_mut<'a>(doc: &'a mut Document, parent: Option<&str>) -> Option<&'a mut Vec<Node>> {
    match parent {
        None => Some(&mut doc.nodes),
        Some(id) => find_mut(&mut doc.nodes, id)?.children_mut(),
    }
}

/// Where a node sits: its parent container (`None` for the top level) and its index there.
pub fn locate(doc: &Document, id: &str) -> Option<(Option<NodeId>, usize)> {
    fn search(nodes: &[Node], parent: Option<&str>, id: &str) -> Option<(Option<NodeId>, usize)> {
        nodes.iter().enumerate().find_map(|(i, n)| {
            if n.id == id {
                Some((parent.map(str::to_owned), i))
            } else {
                search(n.children()?, Some(&n.id), id)
            }
        })
    }
    search(&doc.nodes, None, id)
}

/// Whether `id` is `node` itself or one of its descendants.
pub fn contains(node: &Node, id: &str) -> bool {
    node.id == id || node.children().is_some_and(|c| find(c, id).is_some())
}

/// Inserts `node` into `parent` at `at` (clamped to the end). Returns whether the parent exists.
pub fn insert(doc: &mut Document, parent: Option<&str>, at: usize, node: Node) -> bool {
    let Some(list) = children_mut(doc, parent) else {
        return false;
    };
    list.insert(at.min(list.len()), node);
    true
}

/// Moves a node so it lands at insertion index `at` of `parent` (an index into that list
/// before the node is taken out, as a drop indicator reports it). Refuses to move a container
/// into itself. Returns whether anything changed.
pub fn move_node(doc: &mut Document, id: &str, parent: Option<&str>, at: usize) -> bool {
    let Some((from_parent, from)) = locate(doc, id) else {
        return false;
    };
    if let Some(p) = parent {
        let node = find(&doc.nodes, id).expect("located above");
        if contains(node, p) {
            return false;
        }
    }
    let Some(target_len) = children(doc, parent).map(Vec::len) else {
        return false;
    };
    let same_list = from_parent.as_deref() == parent;
    if at > target_len || (same_list && (at == from || at == from + 1)) {
        return false;
    }
    let node = children_mut(doc, from_parent.as_deref())
        .expect("located above")
        .remove(from);
    let at = if same_list && at > from { at - 1 } else { at };
    insert(doc, parent, at, node)
}

/// Moves a node one step up (`-1`) or down (`1`) among its siblings.
pub fn nudge(doc: &mut Document, id: &str, delta: i32) -> bool {
    let Some((parent, from)) = locate(doc, id) else {
        return false;
    };
    let to = if delta < 0 {
        from.checked_sub(1)
    } else {
        Some(from + 2)
    };
    match to {
        Some(to) => move_node(doc, id, parent.as_deref(), to),
        None => false,
    }
}

/// Replaces the node with the same id. Returns whether it was found.
pub fn replace(doc: &mut Document, node: Node) -> bool {
    match find_mut(&mut doc.nodes, &node.id) {
        Some(slot) => {
            *slot = node;
            true
        }
        None => false,
    }
}

pub fn remove(doc: &mut Document, id: &str) -> Option<Node> {
    let (parent, i) = locate(doc, id)?;
    Some(children_mut(doc, parent.as_deref())?.remove(i))
}

/// Calls `f` on every node, parents before children.
pub fn for_each_mut(nodes: &mut [Node], f: &mut impl FnMut(&mut Node)) {
    for n in nodes {
        f(n);
        if let Some(c) = n.children_mut() {
            for_each_mut(c, f);
        }
    }
}

/// Adds a token of `kind` with a fresh name and starter value. Returns its name.
pub fn add_token(doc: &mut Document, kind: TokenKind) -> String {
    let base = match kind {
        TokenKind::Color => "color",
        TokenKind::Font => "type",
        TokenKind::Space => "space",
    };
    let names = doc.tokens.names(kind);
    let name = (1..)
        .map(|n| format!("{base}-{n}"))
        .find(|n| !names.contains(&n.as_str()))
        .expect("unbounded");
    let t = &mut doc.tokens;
    match kind {
        TokenKind::Color => t.colors.push(ColorToken {
            name: name.clone(),
            light: "#6d4aff".into(),
            dark: None,
        }),
        TokenKind::Font => t.fonts.push(FontToken {
            name: name.clone(),
            family: "system-ui, sans-serif".into(),
            size: 16.0,
            weight: 400,
            line_height: 1.5,
        }),
        TokenKind::Space => t.spacing.push(SpaceToken {
            name: name.clone(),
            value: 32.0,
        }),
    }
    name
}

/// Renames a token and every reference to it. The new name is slugged to a valid CSS
/// identifier; returns `false` (changing nothing) if it is empty, unchanged or taken, or the
/// token is a base token.
pub fn rename_token(doc: &mut Document, kind: TokenKind, old: &str, new: &str) -> bool {
    let new = css::slug(new);
    let names = doc.tokens.names(kind);
    if new.is_empty()
        || new == old
        || names.contains(&new.as_str())
        || !names.contains(&old)
        || kind.is_base(old)
    {
        return false;
    }
    let t = &mut doc.tokens;
    let slot = match kind {
        TokenKind::Color => t.colors.iter_mut().map(|t| &mut t.name).find(|n| *n == old),
        TokenKind::Font => t.fonts.iter_mut().map(|t| &mut t.name).find(|n| *n == old),
        TokenKind::Space => t
            .spacing
            .iter_mut()
            .map(|t| &mut t.name)
            .find(|n| *n == old),
    };
    *slot.expect("checked above") = new.clone();
    rewrite_references(doc, kind, old, Reference::Token(new.clone()));
    true
}

/// Removes a token. References to it keep their look: colors and spacing become the token's
/// literal (light) value, and type falls back to the inherited font. Base tokens stay.
pub fn remove_token(doc: &mut Document, kind: TokenKind, name: &str) -> bool {
    if kind.is_base(name) {
        return false;
    }
    let t = &mut doc.tokens;
    let replacement = match kind {
        TokenKind::Color => {
            take(&mut t.colors, |t| t.name == name).map(|t| Reference::Color(t.light))
        }
        TokenKind::Font => take(&mut t.fonts, |t| t.name == name).map(|_| Reference::None),
        TokenKind::Space => {
            take(&mut t.spacing, |t| t.name == name).map(|t| Reference::Px(t.value))
        }
    };
    let Some(replacement) = replacement else {
        return false;
    };
    rewrite_references(doc, kind, name, replacement);
    true
}

fn take<T>(list: &mut Vec<T>, pred: impl Fn(&T) -> bool) -> Option<T> {
    let i = list.iter().position(pred)?;
    Some(list.remove(i))
}

#[derive(Clone)]
enum Reference {
    Token(String),
    Color(String),
    Px(f64),
    None,
}

fn rewrite_references(doc: &mut Document, kind: TokenKind, name: &str, to: Reference) {
    let color = |value: &mut String| {
        if parse_token_ref(value) == Some(name) {
            *value = match to.clone() {
                Reference::Token(n) => token_ref(&n),
                Reference::Color(c) => c,
                _ => return,
            };
        }
    };
    let length = |value: &mut Length| {
        if matches!(value, Length::Token(n) if n == name) {
            *value = match to.clone() {
                Reference::Token(n) => Length::Token(n),
                Reference::Px(px) => Length::Px(px),
                _ => return,
            };
        }
    };
    for_each_mut(&mut doc.nodes, &mut |n| match kind {
        TokenKind::Color => {
            color(&mut n.style.color);
            if let Some(bg) = &mut n.style.background {
                color(bg);
            }
        }
        TokenKind::Font => {
            if n.style.font.as_deref() == Some(name) {
                n.style.font = match to.clone() {
                    Reference::Token(new) => Some(new),
                    _ => None,
                };
            }
        }
        TokenKind::Space => {
            length(&mut n.style.padding);
            if let NodeKind::Container { layout, .. } = &mut n.kind {
                length(&mut layout.gap);
            }
        }
    });
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

    fn ids(nodes: &[Node]) -> Vec<&str> {
        nodes.iter().map(|n| n.id.as_str()).collect()
    }

    /// `a`, then row `r` holding `x` and `y`, then `b`.
    fn nested() -> Document {
        let mut d = doc(&["a", "b"]);
        let mut row = new_node("row", "r".into()).unwrap();
        row.children_mut().unwrap().extend([
            new_node("text", "x".into()).unwrap(),
            new_node("text", "y".into()).unwrap(),
        ]);
        insert(&mut d, None, 1, row);
        d
    }

    fn row(d: &Document) -> Vec<&str> {
        ids(children(d, Some("r")).unwrap())
    }

    #[test]
    fn move_down_to_insertion_index() {
        let mut d = doc(&["a", "b", "c"]);
        assert!(move_node(&mut d, "a", None, 3));
        assert_eq!(ids(&d.nodes), ["b", "c", "a"]);
    }

    #[test]
    fn move_up() {
        let mut d = doc(&["a", "b", "c"]);
        assert!(move_node(&mut d, "c", None, 0));
        assert_eq!(ids(&d.nodes), ["c", "a", "b"]);
    }

    #[test]
    fn drop_on_own_slot_is_a_no_op() {
        let mut d = doc(&["a", "b"]);
        assert!(!move_node(&mut d, "a", None, 1));
        assert!(!move_node(&mut d, "a", None, 0));
        assert_eq!(ids(&d.nodes), ["a", "b"]);
    }

    #[test]
    fn nudge_stops_at_the_ends() {
        let mut d = doc(&["a", "b", "c"]);
        assert!(!nudge(&mut d, "a", -1));
        assert!(nudge(&mut d, "a", 1));
        assert_eq!(ids(&d.nodes), ["b", "a", "c"]);
        assert!(nudge(&mut d, "c", -1));
        assert_eq!(ids(&d.nodes), ["b", "c", "a"]);
        assert!(!nudge(&mut d, "a", 1));
    }

    #[test]
    fn insert_clamps_and_remove_returns_node() {
        let mut d = doc(&["a"]);
        assert!(insert(
            &mut d,
            None,
            99,
            new_node("spacer", "s".into()).unwrap()
        ));
        assert_eq!(ids(&d.nodes), ["a", "s"]);
        assert_eq!(remove(&mut d, "a").map(|n| n.id), Some("a".into()));
        assert_eq!(ids(&d.nodes), ["s"]);
    }

    #[test]
    fn finds_and_removes_nested_nodes() {
        let mut d = nested();
        assert_eq!(locate(&d, "y"), Some((Some("r".into()), 1)));
        assert_eq!(find(&d.nodes, "x").map(|n| n.id.as_str()), Some("x"));
        assert!(remove(&mut d, "x").is_some());
        assert_eq!(row(&d), ["y"]);
        assert!(!insert(
            &mut d,
            Some("a"),
            0,
            new_node("text", "z".into()).unwrap()
        ));
    }

    #[test]
    fn moves_between_containers() {
        let mut d = nested();
        assert!(move_node(&mut d, "a", Some("r"), 1));
        assert_eq!(ids(&d.nodes), ["r", "b"]);
        assert_eq!(row(&d), ["x", "a", "y"]);
        assert!(move_node(&mut d, "y", None, 0));
        assert_eq!(ids(&d.nodes), ["y", "r", "b"]);
        assert!(nudge(&mut d, "x", 1));
        assert_eq!(row(&d), ["a", "x"]);
    }

    #[test]
    fn will_not_move_a_container_into_itself() {
        let mut d = nested();
        let mut inner = new_node("grid", "g".into()).unwrap();
        inner
            .children_mut()
            .unwrap()
            .push(new_node("text", "deep".into()).unwrap());
        insert(&mut d, Some("r"), 0, inner);
        assert!(!move_node(&mut d, "r", Some("r"), 0));
        assert!(!move_node(&mut d, "r", Some("g"), 0));
        assert!(move_node(&mut d, "deep", None, 0));
        assert_eq!(ids(&d.nodes), ["deep", "a", "r", "b"]);
    }

    #[test]
    fn undo_and_redo_walk_history() {
        let mut h = History::default();
        let mut d = doc(&["a"]);
        h.record(d.clone());
        insert(&mut d, None, 1, new_node("text", "b".into()).unwrap());

        let undone = h.undo(&d).unwrap();
        assert_eq!(ids(&undone.nodes), ["a"]);
        let redone = h.redo(&undone).unwrap();
        assert_eq!(ids(&redone.nodes), ["a", "b"]);
        assert!(h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn palette_types_all_build() {
        assert!(new_node("carousel", "x".into()).is_none());
        for (t, _) in PALETTE {
            let kind = new_node(t, "x".into()).unwrap().kind.type_name();
            assert!(kind == *t || kind == "container", "{t} built a {kind}");
        }
    }

    #[test]
    fn renaming_a_token_updates_references() {
        let mut d = nested();
        d.nodes[0].style.background = Some("{muted}".into());
        assert!(rename_token(&mut d, TokenKind::Color, "muted", "Soft Gray"));
        assert_eq!(d.nodes[0].style.background.as_deref(), Some("{soft-gray}"));
        assert!(d.tokens.colors.iter().any(|t| t.name == "soft-gray"));
        assert!(!rename_token(
            &mut d,
            TokenKind::Color,
            "surface",
            "soft-gray"
        ));
        assert!(!rename_token(&mut d, TokenKind::Color, "accent", "brand"));

        assert!(rename_token(&mut d, TokenKind::Space, "md", "medium"));
        let NodeKind::Container { layout, .. } = &find(&d.nodes, "r").unwrap().kind else {
            panic!("r is a row");
        };
        assert_eq!(layout.gap, Length::Token("medium".into()));
        assert_eq!(
            find(&d.nodes, "x").unwrap().style.padding,
            Length::Token("medium".into())
        );

        assert!(rename_token(&mut d, TokenKind::Font, "body", "copy"));
        assert_eq!(
            find(&d.nodes, "y").unwrap().style.font.as_deref(),
            Some("copy")
        );
    }

    #[test]
    fn removing_a_token_keeps_the_look() {
        let mut d = nested();
        assert!(!remove_token(&mut d, TokenKind::Color, "text"));
        d.nodes[0].style.color = "{muted}".into();
        assert!(remove_token(&mut d, TokenKind::Color, "muted"));
        assert_eq!(d.nodes[0].style.color, "#6b6780");
        assert!(remove_token(&mut d, TokenKind::Space, "md"));
        assert_eq!(find(&d.nodes, "x").unwrap().style.padding, Length::Px(16.0));
        assert!(remove_token(&mut d, TokenKind::Font, "body"));
        assert_eq!(find(&d.nodes, "x").unwrap().style.font, None);
        assert!(!remove_token(&mut d, TokenKind::Font, "body"));
    }

    #[test]
    fn added_tokens_get_unique_names() {
        let mut d = Document::default();
        assert_eq!(add_token(&mut d, TokenKind::Color), "color-1");
        assert_eq!(add_token(&mut d, TokenKind::Color), "color-2");
        assert_eq!(add_token(&mut d, TokenKind::Space), "space-1");
    }
}
