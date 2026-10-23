use serde::Deserialize;

use vellum_export_core::lower::{lower_site, pascal_case, print_css, OutputNode, VOID_ELEMENTS};
use vellum_export_core::{
    parse_options, ExportError, ExportOptions, ExportResult, Exporter, OutputFile,
};
use vellum_schema::{Document, Page};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ReactOptions {
    /// Name of the root component, which picks a page by URL path.
    pub component_name: String,
    /// Emit `.tsx` instead of `.jsx`.
    pub typescript: bool,
}

impl Default for ReactOptions {
    fn default() -> Self {
        Self {
            component_name: "App".into(),
            typescript: false,
        }
    }
}

pub struct ReactExporter;

impl Exporter for ReactExporter {
    fn id(&self) -> &'static str {
        "react"
    }

    fn label(&self) -> &'static str {
        "React"
    }

    fn export(&self, doc: &Document, options: &ExportOptions) -> Result<ExportResult, ExportError> {
        export_react(doc, &parse_options(self.id(), options)?)
    }
}

/// Writes one component per page under `src/pages/`, a root component that renders the page
/// for the current URL path, and one shared stylesheet. Links between pages stay site paths
/// like `/about`, so any static host that serves the app for every path works.
pub fn export_react(doc: &Document, options: &ReactOptions) -> Result<ExportResult, ExportError> {
    let name = &options.component_name;
    let valid_name = name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_name {
        return Err(ExportError::InvalidOptions {
            target: "react".into(),
            message: format!("componentName must be a PascalCase identifier, got \"{name}\""),
        });
    }
    let ext = if options.typescript { "tsx" } else { "jsx" };
    let site_path = |page: &Page| page.path.clone();
    let site = lower_site(doc, &site_path);
    let css = print_css(&site.rules);

    let mut used = vec![name.clone()];
    used.extend(site.components.iter().map(|c| c.name.clone()));
    let components: Vec<String> = site
        .pages
        .iter()
        .map(|page| {
            let base = pascal_case(&page.name, "Page");
            let mut candidate = base.clone();
            let mut n = 2;
            while used.contains(&candidate) {
                candidate = format!("{base}{n}");
                n += 1;
            }
            used.push(candidate.clone());
            candidate
        })
        .collect();

    let mut files = Vec::new();
    let mut app = Vec::new();
    if options.typescript {
        app.push("import type { ComponentType } from \"react\";".to_owned());
    }
    if !css.is_empty() {
        app.push("import \"./styles.css\";".to_owned());
    }
    for (page, component) in site.pages.iter().zip(&components) {
        app.push(format!("import {component} from \"./pages/{component}\";"));
        let root = OutputNode::Element(page.root.clone());
        let mut lines = imports(&root, "../components");
        lines.extend([
            format!("export default function {component}() {{"),
            "  return (".to_owned(),
            print_jsx(&root, 2),
            "  );".to_owned(),
            "}".to_owned(),
            String::new(),
        ]);
        files.push(OutputFile {
            path: format!("src/pages/{component}.{ext}"),
            contents: lines.join("\n"),
        });
    }
    for component in &site.components {
        let root = OutputNode::Element(component.root.clone());
        let mut lines = imports(&root, ".");
        let params = component
            .props
            .iter()
            .map(|p| format!("{} = {}", p.name, js_string(&p.default)))
            .collect::<Vec<_>>();
        let signature = match (params.is_empty(), options.typescript) {
            (true, _) => String::new(),
            (false, false) => format!("{{ {} }}", params.join(", ")),
            (false, true) => {
                let types = component
                    .props
                    .iter()
                    .map(|p| format!("{}?: string", p.name))
                    .collect::<Vec<_>>();
                format!("{{ {} }}: {{ {} }}", params.join(", "), types.join("; "))
            }
        };
        lines.extend([
            format!("export default function {}({signature}) {{", component.name),
            "  return (".to_owned(),
            print_jsx(&root, 2),
            "  );".to_owned(),
            "}".to_owned(),
            String::new(),
        ]);
        files.push(OutputFile {
            path: format!("src/components/{}.{ext}", component.name),
            contents: lines.join("\n"),
        });
    }
    let routes_type = if options.typescript {
        ": Record<string, ComponentType>"
    } else {
        ""
    };
    app.push(String::new());
    app.push(format!("const routes{routes_type} = {{"));
    for (page, component) in site.pages.iter().zip(&components) {
        app.push(format!("  {}: {component},", js_string(&page.path)));
    }
    app.extend([
        "};".to_owned(),
        String::new(),
        format!("export default function {name}() {{"),
        "  const path = window.location.pathname.replace(/\\/+$/, \"\") || \"/\";".to_owned(),
        format!("  const Page = routes[path] ?? {};", components[0]),
        "  return <Page />;".to_owned(),
        "}".to_owned(),
        String::new(),
    ]);
    files.insert(
        0,
        OutputFile {
            path: format!("src/{name}.{ext}"),
            contents: app.join("\n"),
        },
    );
    if !css.is_empty() {
        files.push(OutputFile {
            path: "src/styles.css".into(),
            contents: css,
        });
    }
    Ok(ExportResult {
        target: "react".into(),
        files,
    })
}

/// Import lines for every component used in `root`, then a blank line; nothing if none are.
fn imports(root: &OutputNode, dir: &str) -> Vec<String> {
    fn collect(node: &OutputNode, out: &mut Vec<String>) {
        match node {
            OutputNode::Instance(i) if !out.contains(&i.component) => out.push(i.component.clone()),
            OutputNode::Element(el) => el.children.iter().for_each(|c| collect(c, out)),
            _ => {}
        }
    }
    let mut names = Vec::new();
    collect(root, &mut names);
    let mut lines: Vec<_> = names
        .iter()
        .map(|n| format!("import {n} from \"{dir}/{n}\";"))
        .collect();
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn print_jsx(node: &OutputNode, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    let el = match node {
        OutputNode::Text(text) => return format!("{pad}{}", jsx_text(text)),
        OutputNode::Prop(prop) => return format!("{pad}{{{prop}}}"),
        OutputNode::Instance(instance) => {
            let props: String = instance
                .props
                .iter()
                .map(|(name, value)| format!(" {name}={}", jsx_attr_value(value)))
                .collect();
            return format!("{pad}<{}{props} />", instance.component);
        }
        OutputNode::Element(el) => el,
    };

    let mut attrs: String = el
        .attrs
        .iter()
        .map(|(name, value)| format!(" {}={}", jsx_attr_name(name), jsx_attr_value(value)))
        .collect();
    for (name, prop) in &el.prop_attrs {
        attrs.push_str(&format!(" {}={{{prop}}}", jsx_attr_name(name)));
    }
    let open = format!("<{}{attrs}", el.tag);
    match el.children.as_slice() {
        [] => format!("{pad}{open} />"),
        _ if VOID_ELEMENTS.contains(&el.tag) => format!("{pad}{open} />"),
        [OutputNode::Text(text)] => format!("{pad}{open}>{}</{}>", jsx_text(text), el.tag),
        [OutputNode::Prop(prop)] => format!("{pad}{open}>{{{prop}}}</{}>", el.tag),
        children => {
            let mut lines = vec![format!("{pad}{open}>")];
            lines.extend(children.iter().map(|child| print_jsx(child, depth + 1)));
            lines.push(format!("{pad}</{}>", el.tag));
            lines.join("\n")
        }
    }
}

fn jsx_attr_name(name: &str) -> String {
    match name {
        "class" => return "className".into(),
        "for" => return "htmlFor".into(),
        _ if name.starts_with("aria-") || name.starts_with("data-") => return name.into(),
        _ => {}
    }
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn jsx_attr_value(value: &str) -> String {
    if value.contains(['"', '&', '{', '}', '\n', '\\']) {
        format!("{{{}}}", js_string(value))
    } else {
        format!("\"{value}\"")
    }
}

fn jsx_text(value: &str) -> String {
    // JSX collapses surrounding whitespace and treats braces/angle brackets as syntax.
    let needs_expression = value.is_empty()
        || value.contains(['{', '}', '<', '>', '&', '\n'])
        || value.starts_with(char::is_whitespace)
        || value.ends_with(char::is_whitespace);
    if needs_expression {
        format!("{{{}}}", js_string(value))
    } else {
        value.to_owned()
    }
}

/// A JSON string literal is a valid JS string literal.
fn js_string(value: &str) -> String {
    serde_json::to_string(value).expect("strings always serialize")
}
