//! Reusable components: creating them from a block, their props and bindings, and instances.

use std::collections::BTreeMap;

use vellum_schema::{Component, Document, Node, NodeId, NodeKind, PropDef, PropKind};

use crate::{children_mut, find, find_mut, for_each_mut, locate};

/// Every prop's value for one instance: the instance's own value, or the prop's default.
pub fn prop_values(
    component: &Component,
    props: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    component
        .props
        .iter()
        .map(|p| {
            let value = props.get(&p.name).unwrap_or(&p.default);
            (p.name.clone(), value.clone())
        })
        .collect()
}

/// The component's nodes with bound fields showing `props` (or the defaults). Ids are the
/// component's own, so the result is for showing, not editing.
pub fn expand(component: &Component, props: &BTreeMap<String, String>) -> Vec<Node> {
    let values = prop_values(component, props);
    let mut nodes = component.nodes.clone();
    for_each_mut(&mut nodes, &mut |n| {
        for (field, prop) in &n.bindings {
            if let Some(value) = values.get(prop) {
                n.kind.set_field(field, value.clone());
            }
        }
        n.bindings.clear();
    });
    nodes
}

/// Whether the tree `into` (a page or component id) may not hold an instance of `component`,
/// because the component would end up containing itself.
pub fn would_cycle(doc: &Document, into: &str, component: &str) -> bool {
    fn uses(doc: &Document, component: &str, target: &str, seen: &mut Vec<String>) -> bool {
        if component == target {
            return true;
        }
        if seen.iter().any(|s| s == component) {
            return false;
        }
        seen.push(component.to_owned());
        let Some(c) = doc.component(component) else {
            return false;
        };
        let mut found = false;
        let mut nodes = c.nodes.clone();
        for_each_mut(&mut nodes, &mut |n| {
            if let NodeKind::Instance { component, .. } = &n.kind {
                found = found || uses(doc, component, target, &mut seen.clone());
            }
        });
        found
    }
    doc.component(into).is_some() && uses(doc, component, into, &mut Vec::new())
}

/// A new instance of `component` with default props, or `None` if there is no such component.
pub fn new_instance(doc: &Document, component: &str, id: String) -> Option<Node> {
    doc.component(component)?;
    Some(Node::new(
        id,
        NodeKind::Instance {
            component: component.to_owned(),
            props: BTreeMap::new(),
        },
    ))
}

/// How many instances of `component` the site has, on pages and inside other components.
pub fn instance_count(doc: &Document, component: &str) -> usize {
    let mut doc = doc.clone();
    let mut count = 0;
    for tree in doc.trees_mut() {
        for_each_mut(tree, &mut |n| {
            if matches!(&n.kind, NodeKind::Instance { component: c, .. } if c == component) {
                count += 1;
            }
        });
    }
    count
}

/// Turns the node `node` in the tree `tree` into a new component named after its position
/// (`Component 1`, ...), leaving an instance in its place. Returns the component's id.
pub fn create_component(
    doc: &mut Document,
    tree: &str,
    node: &str,
    component_id: String,
    instance_id: String,
) -> Option<NodeId> {
    let nodes = doc.tree_mut(tree)?;
    let (parent, i) = locate(nodes, node)?;
    let list = children_mut(nodes, parent.as_deref())?;
    let original = std::mem::replace(
        &mut list[i],
        Node::new(
            instance_id,
            NodeKind::Instance {
                component: component_id.clone(),
                props: BTreeMap::new(),
            },
        ),
    );
    let name = (1..)
        .map(|n| format!("Component {n}"))
        .find(|name| !doc.components.iter().any(|c| c.name == *name))
        .expect("unbounded");
    doc.components.push(Component {
        id: component_id.clone(),
        name,
        props: Vec::new(),
        nodes: vec![original],
    });
    Some(component_id)
}

/// Turns user input into a prop name: a camelCase JavaScript identifier that is not a reserved
/// word, or `None` if nothing usable is left.
pub fn prop_identifier(input: &str) -> Option<String> {
    let mut out = String::new();
    for (i, word) in input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .enumerate()
    {
        let mut chars = word.chars();
        let first = chars.next().expect("non-empty");
        if i == 0 {
            out.push(first.to_ascii_lowercase());
        } else {
            out.push(first.to_ascii_uppercase());
        }
        out.extend(chars);
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'p');
    }
    const RESERVED: &[&str] = &[
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "null",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
        "let",
        "static",
        "await",
        "key",
        "ref",
    ];
    (!out.is_empty() && !RESERVED.contains(&out.as_str())).then_some(out)
}

/// Adds a prop of `kind` with a fresh name and starter default. Returns its name.
pub fn add_prop(doc: &mut Document, component: &str, kind: PropKind) -> Option<String> {
    let c = doc.component_mut(component)?;
    let name = (1..)
        .map(|n| match n {
            1 => kind.as_str().to_owned(),
            n => format!("{}{n}", kind.as_str()),
        })
        .find(|name| c.prop(name).is_none())
        .expect("unbounded");
    let default = match kind {
        PropKind::Text => "Text",
        PropKind::Link => "#",
        PropKind::Image => crate::PLACEHOLDER_IMAGE,
    };
    c.props.push(PropDef {
        name: name.clone(),
        kind,
        default: default.into(),
    });
    Some(name)
}

/// Calls `f` on every instance of `component`, wherever it is.
fn for_each_instance(
    doc: &mut Document,
    component: &str,
    f: &mut impl FnMut(&mut BTreeMap<String, String>),
) {
    for tree in doc.trees_mut() {
        for_each_mut(tree, &mut |n| {
            if let NodeKind::Instance {
                component: c,
                props,
            } = &mut n.kind
            {
                if c == component {
                    f(props);
                }
            }
        });
    }
}

/// Renames a prop, along with the fields bound to it and every instance's value for it.
/// Returns `false` (changing nothing) if the new name is unusable, unchanged or taken.
pub fn rename_prop(doc: &mut Document, component: &str, old: &str, new: &str) -> bool {
    let Some(new) = prop_identifier(new) else {
        return false;
    };
    let Some(c) = doc.component_mut(component) else {
        return false;
    };
    if new == old || c.prop(&new).is_some() {
        return false;
    }
    let Some(prop) = c.props.iter_mut().find(|p| p.name == old) else {
        return false;
    };
    prop.name = new.clone();
    for_each_mut(&mut c.nodes, &mut |n| {
        for bound in n.bindings.values_mut() {
            if bound == old {
                *bound = new.clone();
            }
        }
    });
    for_each_instance(doc, component, &mut |props| {
        if let Some(value) = props.remove(old) {
            props.insert(new.clone(), value);
        }
    });
    true
}

/// Removes a prop. Fields bound to it keep showing its default as their own value.
pub fn remove_prop(doc: &mut Document, component: &str, name: &str) -> bool {
    let Some(c) = doc.component_mut(component) else {
        return false;
    };
    let Some(i) = c.props.iter().position(|p| p.name == name) else {
        return false;
    };
    let prop = c.props.remove(i);
    for_each_mut(&mut c.nodes, &mut |n| {
        let fields: Vec<_> = n
            .bindings
            .iter()
            .filter(|(_, p)| **p == prop.name)
            .map(|(f, _)| f.clone())
            .collect();
        for field in fields {
            n.bindings.remove(&field);
            n.kind.set_field(&field, prop.default.clone());
        }
    });
    for_each_instance(doc, component, &mut |props| {
        props.remove(name);
    });
    true
}

/// Binds a content field of a node inside `component` to a prop, or unbinds it for `None`.
/// Returns whether anything changed.
pub fn bind(
    doc: &mut Document,
    component: &str,
    node: &str,
    field: &str,
    prop: Option<&str>,
) -> bool {
    let Some(c) = doc.component_mut(component) else {
        return false;
    };
    if prop.is_some_and(|p| c.prop(p).is_none()) {
        return false;
    }
    let Some(n) = find_mut(&mut c.nodes, node) else {
        return false;
    };
    if !n.kind.bindable_fields().iter().any(|(f, _)| *f == field) {
        return false;
    }
    let before = n.bindings.get(field).cloned();
    match prop {
        Some(p) => n.bindings.insert(field.to_owned(), p.to_owned()),
        None => n.bindings.remove(field),
    };
    before.as_deref() != prop
}

/// Gives a node and everything inside it fresh ids.
fn reassign_ids(nodes: &mut [Node], next_id: &mut impl FnMut() -> String) {
    for_each_mut(nodes, &mut |n| n.id = next_id());
}

/// Replaces an instance in `tree` with a copy of the component's blocks, with its prop values
/// filled in. Returns whether it was found.
pub fn detach(
    doc: &mut Document,
    tree: &str,
    instance: &str,
    next_id: &mut impl FnMut() -> String,
) -> bool {
    let Some(nodes) = doc.tree(tree) else {
        return false;
    };
    let Some(NodeKind::Instance { component, props }) = find(nodes, instance).map(|n| &n.kind)
    else {
        return false;
    };
    let Some(c) = doc.component(component) else {
        return false;
    };
    let mut expanded = expand(c, props);
    reassign_ids(&mut expanded, next_id);
    let nodes = doc.tree_mut(tree).expect("found above");
    let (parent, i) = locate(nodes, instance).expect("found above");
    let list = children_mut(nodes, parent.as_deref()).expect("found above");
    list.splice(i..=i, expanded);
    true
}

/// Removes a component, detaching every instance of it first so pages keep their content.
pub fn remove_component(
    doc: &mut Document,
    component: &str,
    next_id: &mut impl FnMut() -> String,
) -> bool {
    if doc.component(component).is_none() {
        return false;
    }
    loop {
        // Find one remaining instance (on a page or in another component) and detach it.
        let mut target = None;
        let tree_ids: Vec<NodeId> = doc
            .pages
            .iter()
            .map(|p| p.id.clone())
            .chain(doc.components.iter().map(|c| c.id.clone()))
            .filter(|id| id != component)
            .collect();
        for tree in tree_ids {
            let mut nodes = doc.tree(&tree).cloned().unwrap_or_default();
            for_each_mut(&mut nodes, &mut |n| {
                if matches!(&n.kind, NodeKind::Instance { component: c, .. } if c == component) {
                    target.get_or_insert((tree.clone(), n.id.clone()));
                }
            });
            if target.is_some() {
                break;
            }
        }
        match target {
            Some((tree, instance)) => {
                detach(doc, &tree, &instance, next_id);
            }
            None => break,
        }
    }
    doc.components.retain(|c| c.id != component);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_node;

    /// Home holds a heading `h` and a row `r`; the row is made into a component with a bound
    /// button.
    fn site() -> Document {
        let mut d = Document::default();
        let mut row = new_node("row", "r".into()).unwrap();
        row.children_mut()
            .unwrap()
            .push(new_node("button", "b".into()).unwrap());
        d.pages[0].nodes = vec![new_node("heading", "h".into()).unwrap(), row];
        create_component(&mut d, "home", "r", "c".into(), "i".into()).unwrap();
        d
    }

    fn counter() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("n{n}")
        }
    }

    #[test]
    fn creating_a_component_leaves_an_instance() {
        let d = site();
        assert_eq!(d.components[0].name, "Component 1");
        assert_eq!(d.components[0].nodes[0].id, "r");
        assert!(matches!(
            &d.pages[0].nodes[1].kind,
            NodeKind::Instance { component, .. } if component == "c"
        ));
        assert_eq!(instance_count(&d, "c"), 1);
    }

    #[test]
    fn bound_fields_show_instance_values_or_defaults() {
        let mut d = site();
        let label = add_prop(&mut d, "c", PropKind::Text).unwrap();
        let link = add_prop(&mut d, "c", PropKind::Link).unwrap();
        assert_eq!((label.as_str(), link.as_str()), ("text", "link"));
        assert!(bind(&mut d, "c", "b", "label", Some(&label)));
        assert!(bind(&mut d, "c", "b", "href", Some(&link)));
        assert!(
            !bind(&mut d, "c", "b", "src", Some(&label)),
            "buttons have no src"
        );
        assert!(!bind(&mut d, "c", "b", "label", Some("missing")));

        let c = d.component("c").unwrap();
        let props = BTreeMap::from([("text".to_owned(), "Buy".to_owned())]);
        let expanded = expand(c, &props);
        let button = find(&expanded, "b").unwrap();
        assert_eq!(button.kind.field("label"), Some("Buy"));
        assert_eq!(button.kind.field("href"), Some("#"));
        assert!(button.bindings.is_empty());
    }

    #[test]
    fn renaming_and_removing_props_follow_through() {
        let mut d = site();
        add_prop(&mut d, "c", PropKind::Text).unwrap();
        bind(&mut d, "c", "b", "label", Some("text"));
        if let NodeKind::Instance { props, .. } = &mut d.pages[0].nodes[1].kind {
            props.insert("text".into(), "Hello".into());
        }
        assert!(rename_prop(&mut d, "c", "text", "Button label"));
        assert!(
            !rename_prop(&mut d, "c", "buttonLabel", "class"),
            "reserved word"
        );
        let c = d.component("c").unwrap();
        assert_eq!(c.props[0].name, "buttonLabel");
        assert_eq!(
            find(&c.nodes, "b").unwrap().bindings["label"],
            "buttonLabel"
        );
        let NodeKind::Instance { props, .. } = &d.pages[0].nodes[1].kind else {
            panic!("instance");
        };
        assert_eq!(props["buttonLabel"], "Hello");

        assert!(remove_prop(&mut d, "c", "buttonLabel"));
        let b = find(&d.component("c").unwrap().nodes, "b").unwrap();
        assert!(b.bindings.is_empty());
        assert_eq!(b.kind.field("label"), Some("Text"));
    }

    #[test]
    fn components_cannot_contain_themselves() {
        let mut d = site();
        create_component(&mut d, "home", "h", "c2".into(), "i2".into());
        assert!(!would_cycle(&d, "home", "c"), "pages can hold anything");
        assert!(would_cycle(&d, "c", "c"));
        assert!(!would_cycle(&d, "c", "c2"));
        let inner = new_instance(&d, "c2", "x".into()).unwrap();
        d.component_mut("c").unwrap().nodes.push(inner);
        assert!(would_cycle(&d, "c2", "c"), "c holds c2, so c2 can't hold c");
    }

    #[test]
    fn detaching_and_removing_keep_the_content() {
        let mut d = site();
        let second = new_instance(&d, "c", "i2".into()).unwrap();
        d.pages[0].nodes.push(second);
        let mut ids = counter();
        assert!(detach(&mut d, "home", "i", &mut ids));
        assert_eq!(d.pages[0].nodes[1].kind.type_name(), "container");
        assert_eq!(d.pages[0].nodes[1].id, "n1");
        assert!(remove_component(&mut d, "c", &mut ids));
        assert!(d.components.is_empty());
        assert_eq!(d.pages[0].nodes[2].kind.type_name(), "container");
        assert_eq!(
            d.pages[0].nodes[2].id, "n3",
            "fresh ids, unique on the page"
        );
    }

    #[test]
    fn prop_names_become_identifiers() {
        assert_eq!(
            prop_identifier("Button label").as_deref(),
            Some("buttonLabel")
        );
        assert_eq!(prop_identifier("2nd line").as_deref(), Some("p2ndLine"));
        assert_eq!(prop_identifier("--"), None);
        assert_eq!(prop_identifier("default"), None);
    }
}
