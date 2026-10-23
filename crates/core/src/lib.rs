//! Editing operations on a [`Document`], kept free of any UI so they can be tested natively.

mod components;

pub use components::*;

use vellum_schema::{
    css, parse_token_ref, token_ref, ColorToken, Document, Drawing, FontToken, Layout, Length,
    Node, NodeId, NodeKind, Page, SpaceToken, TokenKind,
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
    ("nav", "Navigation"),
];

/// Inline placeholder so a fresh image block renders without network access.
pub const PLACEHOLDER_IMAGE: &str = "data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%22800%22%20height%3D%22400%22%3E%3Crect%20width%3D%22800%22%20height%3D%22400%22%20fill%3D%22%23eee9ff%22%2F%3E%3Cpath%20d%3D%22M260%20300l120-140%2090%20100%2060-60%20110%20100z%22%20fill%3D%22%23c7b8ff%22%2F%3E%3Ccircle%20cx%3D%22560%22%20cy%3D%22130%22%20r%3D%2240%22%20fill%3D%22%23c7b8ff%22%2F%3E%3C%2Fsvg%3E";

/// A new block of the given palette type with starter content, or `None` for an unknown type.
pub fn new_node(type_name: &str, id: String) -> Option<Node> {
    let mut font = None;
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
            font = Some("body".into());
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
        "nav" => NodeKind::Nav {},
        _ => return None,
    };
    let mut node = Node::new(id, kind);
    node.style.font = font;
    Some(node)
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
    let mut about_link = new_node("button", next_id()).unwrap();
    about_link.kind = NodeKind::Button {
        label: "About us".into(),
        href: "/about".into(),
    };
    if let Some(children) = row.children_mut() {
        children.extend([about_link, secondary]);
    }
    let home = &mut doc.pages[0];
    home.nodes = vec![new_node("nav", next_id()).unwrap(), heading, text, row];

    let about = add_page(&mut doc, next_id());
    let mut about_heading = new_node("heading", next_id()).unwrap();
    about_heading.kind = NodeKind::Heading {
        text: "About".into(),
        level: 1,
    };
    let mut about_text = new_node("text", next_id()).unwrap();
    about_text.kind = NodeKind::Text {
        text: "Every page shares the site's styles. Add pages under Pages, and link to them with paths like /about.".into(),
    };
    let page = doc.page_mut(&about).expect("just added");
    page.name = "About".into();
    page.path = "/about".into();
    page.nodes = vec![
        new_node("nav", next_id()).unwrap(),
        about_heading,
        about_text,
    ];
    doc
}

/// Adds a blank page after the others, with a fresh name and path. Returns its id.
pub fn add_page(doc: &mut Document, id: String) -> NodeId {
    let n = (doc.pages.len() + 1..)
        .find(|n| {
            let path = format!("/page-{n}");
            !doc.pages.iter().any(|p| p.path == path)
        })
        .expect("unbounded");
    doc.pages.push(Page {
        id: id.clone(),
        name: format!("Page {n}"),
        path: format!("/page-{n}"),
        nodes: Vec::new(),
    });
    id
}

/// Turns user input like `About Us` or `/about-us/` into a page path like `/about-us`.
pub fn normalize_path(input: &str) -> String {
    format!("/{}", css::slug(input))
}

/// Sets a page's path from user input. The home page stays at `/`, and other pages need a
/// non-empty path no other page has. Returns whether it changed.
pub fn set_page_path(doc: &mut Document, page: &str, input: &str) -> bool {
    let path = normalize_path(input);
    let taken = doc.pages.iter().any(|p| p.path == path && p.id != page);
    match doc.page_mut(page) {
        // `/index` would export to the same file as the home page.
        Some(p) if !p.is_home() && path != "/" && path != "/index" && !taken && p.path != path => {
            p.path = path;
            true
        }
        _ => false,
    }
}

/// Removes a page. The home page can't be removed.
pub fn remove_page(doc: &mut Document, page: &str) -> Option<Page> {
    let i = doc
        .pages
        .iter()
        .position(|p| p.id == page && !p.is_home())?;
    Some(doc.pages.remove(i))
}

/// Moves a page one step earlier (`-1`) or later (`1`) in navigation. The home page stays first.
pub fn nudge_page(doc: &mut Document, page: &str, delta: i32) -> bool {
    let Some(from) = doc.pages.iter().position(|p| p.id == page) else {
        return false;
    };
    let to = from as i64 + delta.signum() as i64;
    let in_range = to >= 0 && (to as usize) < doc.pages.len();
    if !in_range || doc.pages[from].is_home() || doc.pages[to as usize].is_home() {
        return false;
    }
    doc.pages.swap(from, to as usize);
    true
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

// The tree operations below take a page's top-level `nodes`, so they work on any page.

/// The child list of `parent`, or the top level for `None`. `None` if `parent` is missing or
/// not a container.
pub fn children<'a>(nodes: &'a [Node], parent: Option<&str>) -> Option<&'a [Node]> {
    match parent {
        None => Some(nodes),
        Some(id) => find(nodes, id)?.children().map(Vec::as_slice),
    }
}

pub fn children_mut<'a>(
    nodes: &'a mut Vec<Node>,
    parent: Option<&str>,
) -> Option<&'a mut Vec<Node>> {
    match parent {
        None => Some(nodes),
        Some(id) => find_mut(nodes, id)?.children_mut(),
    }
}

/// Where a node sits: its parent container (`None` for the top level) and its index there.
pub fn locate(nodes: &[Node], id: &str) -> Option<(Option<NodeId>, usize)> {
    fn search(nodes: &[Node], parent: Option<&str>, id: &str) -> Option<(Option<NodeId>, usize)> {
        nodes.iter().enumerate().find_map(|(i, n)| {
            if n.id == id {
                Some((parent.map(str::to_owned), i))
            } else {
                search(n.children()?, Some(&n.id), id)
            }
        })
    }
    search(nodes, None, id)
}

/// Whether `id` is `node` itself or one of its descendants.
pub fn contains(node: &Node, id: &str) -> bool {
    node.id == id || node.children().is_some_and(|c| find(c, id).is_some())
}

/// Inserts `node` into `parent` at `at` (clamped to the end). Returns whether the parent exists.
pub fn insert(nodes: &mut Vec<Node>, parent: Option<&str>, at: usize, node: Node) -> bool {
    let Some(list) = children_mut(nodes, parent) else {
        return false;
    };
    list.insert(at.min(list.len()), node);
    true
}

/// Moves a node so it lands at insertion index `at` of `parent` (an index into that list
/// before the node is taken out, as a drop indicator reports it). Refuses to move a container
/// into itself. Returns whether anything changed.
pub fn move_node(nodes: &mut Vec<Node>, id: &str, parent: Option<&str>, at: usize) -> bool {
    let Some((from_parent, from)) = locate(nodes, id) else {
        return false;
    };
    if let Some(p) = parent {
        let node = find(nodes, id).expect("located above");
        if contains(node, p) {
            return false;
        }
    }
    let Some(target_len) = children(nodes, parent).map(<[Node]>::len) else {
        return false;
    };
    let same_list = from_parent.as_deref() == parent;
    if at > target_len || (same_list && (at == from || at == from + 1)) {
        return false;
    }
    let node = children_mut(nodes, from_parent.as_deref())
        .expect("located above")
        .remove(from);
    let at = if same_list && at > from { at - 1 } else { at };
    insert(nodes, parent, at, node)
}

/// Moves a node one step up (`-1`) or down (`1`) among its siblings.
pub fn nudge(nodes: &mut Vec<Node>, id: &str, delta: i32) -> bool {
    let Some((parent, from)) = locate(nodes, id) else {
        return false;
    };
    let to = if delta < 0 {
        from.checked_sub(1)
    } else {
        Some(from + 2)
    };
    match to {
        Some(to) => move_node(nodes, id, parent.as_deref(), to),
        None => false,
    }
}

/// Replaces the node with the same id. Returns whether it was found.
pub fn replace(nodes: &mut [Node], node: Node) -> bool {
    match find_mut(nodes, &node.id) {
        Some(slot) => {
            *slot = node;
            true
        }
        None => false,
    }
}

pub fn remove(nodes: &mut Vec<Node>, id: &str) -> Option<Node> {
    let (parent, i) = locate(nodes, id)?;
    Some(children_mut(nodes, parent.as_deref())?.remove(i))
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
    let font = |value: &mut Option<String>| {
        if value.as_deref() == Some(name) {
            *value = match to.clone() {
                Reference::Token(new) => Some(new),
                _ => None,
            };
        }
    };
    let optional_length = |value: &mut Option<Length>| {
        if let Some(l) = value {
            length(l);
        }
    };
    let mut visit = |n: &mut Node| match kind {
        TokenKind::Color => {
            color(&mut n.style.color);
            if let Some(bg) = &mut n.style.background {
                color(bg);
            }
        }
        TokenKind::Font => {
            font(&mut n.style.font);
            for o in n.responsive.values_mut() {
                font(&mut o.font);
            }
        }
        TokenKind::Space => {
            length(&mut n.style.padding);
            if let NodeKind::Container { layout, .. } = &mut n.kind {
                length(&mut layout.gap);
            }
            for o in n.responsive.values_mut() {
                optional_length(&mut o.padding);
                optional_length(&mut o.gap);
            }
        }
    };
    for tree in doc.trees_mut() {
        for_each_mut(tree, &mut visit);
    }
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

    fn doc(ids: &[&str]) -> Vec<Node> {
        ids.iter()
            .map(|id| new_node("text", id.to_string()).unwrap())
            .collect()
    }

    /// A one-page site whose home page is `nodes`.
    fn site(nodes: Vec<Node>) -> Document {
        let mut d = Document::default();
        d.pages[0].nodes = nodes;
        d
    }

    fn ids(nodes: &[Node]) -> Vec<&str> {
        nodes.iter().map(|n| n.id.as_str()).collect()
    }

    /// `a`, then row `r` holding `x` and `y`, then `b`.
    fn nested() -> Vec<Node> {
        let mut d = doc(&["a", "b"]);
        let mut row = new_node("row", "r".into()).unwrap();
        row.children_mut().unwrap().extend([
            new_node("text", "x".into()).unwrap(),
            new_node("text", "y".into()).unwrap(),
        ]);
        insert(&mut d, None, 1, row);
        d
    }

    fn row(d: &[Node]) -> Vec<&str> {
        ids(children(d, Some("r")).unwrap())
    }

    #[test]
    fn move_down_to_insertion_index() {
        let mut d = doc(&["a", "b", "c"]);
        assert!(move_node(&mut d, "a", None, 3));
        assert_eq!(ids(&d), ["b", "c", "a"]);
    }

    #[test]
    fn move_up() {
        let mut d = doc(&["a", "b", "c"]);
        assert!(move_node(&mut d, "c", None, 0));
        assert_eq!(ids(&d), ["c", "a", "b"]);
    }

    #[test]
    fn drop_on_own_slot_is_a_no_op() {
        let mut d = doc(&["a", "b"]);
        assert!(!move_node(&mut d, "a", None, 1));
        assert!(!move_node(&mut d, "a", None, 0));
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
        assert!(insert(
            &mut d,
            None,
            99,
            new_node("spacer", "s".into()).unwrap()
        ));
        assert_eq!(ids(&d), ["a", "s"]);
        assert_eq!(remove(&mut d, "a").map(|n| n.id), Some("a".into()));
        assert_eq!(ids(&d), ["s"]);
    }

    #[test]
    fn finds_and_removes_nested_nodes() {
        let mut d = nested();
        assert_eq!(locate(&d, "y"), Some((Some("r".into()), 1)));
        assert_eq!(find(&d, "x").map(|n| n.id.as_str()), Some("x"));
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
        assert_eq!(ids(&d), ["r", "b"]);
        assert_eq!(row(&d), ["x", "a", "y"]);
        assert!(move_node(&mut d, "y", None, 0));
        assert_eq!(ids(&d), ["y", "r", "b"]);
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
        assert_eq!(ids(&d), ["deep", "a", "r", "b"]);
    }

    #[test]
    fn undo_and_redo_walk_history() {
        let mut h = History::default();
        let mut d = site(doc(&["a"]));
        h.record(d.clone());
        insert(
            &mut d.pages[0].nodes,
            None,
            1,
            new_node("text", "b".into()).unwrap(),
        );

        let undone = h.undo(&d).unwrap();
        assert_eq!(ids(&undone.pages[0].nodes), ["a"]);
        let redone = h.redo(&undone).unwrap();
        assert_eq!(ids(&redone.pages[0].nodes), ["a", "b"]);
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
        let mut d = site(nested());
        d.pages[0].nodes[0].style.background = Some("{muted}".into());
        assert!(rename_token(&mut d, TokenKind::Color, "muted", "Soft Gray"));
        assert_eq!(
            d.pages[0].nodes[0].style.background.as_deref(),
            Some("{soft-gray}")
        );
        assert!(d.tokens.colors.iter().any(|t| t.name == "soft-gray"));
        assert!(!rename_token(
            &mut d,
            TokenKind::Color,
            "surface",
            "soft-gray"
        ));
        assert!(!rename_token(&mut d, TokenKind::Color, "accent", "brand"));

        let x = find_mut(&mut d.pages[0].nodes, "x").unwrap();
        x.responsive.insert(
            vellum_schema::Breakpoint::Mobile,
            vellum_schema::Override {
                padding: Some(Length::Token("md".into())),
                font: Some("body".into()),
                ..Default::default()
            },
        );
        assert!(rename_token(&mut d, TokenKind::Space, "md", "medium"));
        let mobile = &find(&d.pages[0].nodes, "x").unwrap().responsive;
        assert_eq!(
            mobile[&vellum_schema::Breakpoint::Mobile].padding,
            Some(Length::Token("medium".into()))
        );
        let NodeKind::Container { layout, .. } = &find(&d.pages[0].nodes, "r").unwrap().kind else {
            panic!("r is a row");
        };
        assert_eq!(layout.gap, Length::Token("medium".into()));
        assert_eq!(
            find(&d.pages[0].nodes, "x").unwrap().style.padding,
            Length::Token("medium".into())
        );

        assert!(rename_token(&mut d, TokenKind::Font, "body", "copy"));
        let mobile = &find(&d.pages[0].nodes, "x").unwrap().responsive;
        assert_eq!(
            mobile[&vellum_schema::Breakpoint::Mobile].font.as_deref(),
            Some("copy")
        );
        assert_eq!(
            find(&d.pages[0].nodes, "y").unwrap().style.font.as_deref(),
            Some("copy")
        );
    }

    #[test]
    fn removing_a_token_keeps_the_look() {
        let mut d = site(nested());
        assert!(!remove_token(&mut d, TokenKind::Color, "text"));
        d.pages[0].nodes[0].style.color = "{muted}".into();
        assert!(remove_token(&mut d, TokenKind::Color, "muted"));
        assert_eq!(d.pages[0].nodes[0].style.color, "#6b6780");
        assert!(remove_token(&mut d, TokenKind::Space, "md"));
        assert_eq!(
            find(&d.pages[0].nodes, "x").unwrap().style.padding,
            Length::Px(16.0)
        );
        assert!(remove_token(&mut d, TokenKind::Font, "body"));
        assert_eq!(find(&d.pages[0].nodes, "x").unwrap().style.font, None);
        assert!(!remove_token(&mut d, TokenKind::Font, "body"));
    }

    #[test]
    fn added_tokens_get_unique_names() {
        let mut d = Document::default();
        assert_eq!(add_token(&mut d, TokenKind::Color), "color-1");
        assert_eq!(add_token(&mut d, TokenKind::Color), "color-2");
        assert_eq!(add_token(&mut d, TokenKind::Space), "space-1");
    }

    #[test]
    fn token_references_update_on_every_page() {
        let mut d = site(doc(&["a"]));
        let about = add_page(&mut d, "p2".into());
        d.page_mut(&about).unwrap().nodes = doc(&["b"]);
        d.pages[1].nodes[0].style.color = "{muted}".into();
        assert!(rename_token(&mut d, TokenKind::Color, "muted", "quiet"));
        assert_eq!(d.pages[1].nodes[0].style.color, "{quiet}");
    }

    #[test]
    fn pages_get_unique_paths_and_the_home_page_stays_put() {
        let mut d = Document::default();
        let a = add_page(&mut d, "a".into());
        let b = add_page(&mut d, "b".into());
        assert_eq!(d.page(&a).unwrap().path, "/page-2");
        assert_eq!(d.page(&b).unwrap().path, "/page-3");

        assert!(set_page_path(&mut d, &a, "About Us/"));
        assert_eq!(d.page(&a).unwrap().path, "/about-us");
        assert!(!set_page_path(&mut d, &b, "/about-us"), "taken");
        assert!(!set_page_path(&mut d, &b, "///"), "empty");
        assert!(
            !set_page_path(&mut d, &b, "index"),
            "clashes with index.html"
        );
        assert!(!set_page_path(&mut d, "home", "/start"), "home stays at /");

        assert!(!nudge_page(&mut d, &a, -1), "home stays first");
        assert!(nudge_page(&mut d, &a, 1));
        assert_eq!(d.pages[2].id, a);

        assert!(remove_page(&mut d, "home").is_none());
        assert!(remove_page(&mut d, &b).is_some());
        assert_eq!(d.pages.len(), 2);
        // A removed path is free again.
        assert_eq!(add_page(&mut d, "c".into()), "c");
        assert_eq!(d.page("c").unwrap().path, "/page-3");
    }

    #[test]
    fn starter_site_links_its_pages() {
        let mut n = 0;
        let d = starter_document(|| {
            n += 1;
            n.to_string()
        });
        assert_eq!(d.pages.len(), 2);
        let links: Vec<_> = d.pages[0]
            .nodes
            .iter()
            .flat_map(|n| n.children().into_iter().flatten())
            .filter_map(|n| match &n.kind {
                NodeKind::Button { href, .. } => Some(href.as_str()),
                _ => None,
            })
            .collect();
        assert!(links.iter().any(|href| d.page_for_link(href).is_some()));
    }
}
