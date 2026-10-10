use serde::Deserialize;

use vellum_export_core::lower::{lower_document, print_css, OutputNode, VOID_ELEMENTS};
use vellum_export_core::{
    parse_options, ExportError, ExportOptions, ExportResult, Exporter, OutputFile,
};
use vellum_schema::Document;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ReactOptions {
    /// Name of the generated component.
    pub component_name: String,
    /// Emit `.tsx` instead of `.jsx`.
    pub typescript: bool,
}

impl Default for ReactOptions {
    fn default() -> Self {
        Self {
            component_name: "Page".into(),
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
    let page = lower_document(doc);
    let css = print_css(&page.rules);

    let root = OutputNode::Element(page.root);

    let mut lines = Vec::new();
    if !css.is_empty() {
        lines.push(format!("import \"./{name}.css\";"));
        lines.push(String::new());
    }
    lines.extend([
        format!("export default function {name}() {{"),
        "  return (".to_owned(),
        print_jsx(&root, 2),
        "  );".to_owned(),
        "}".to_owned(),
        String::new(),
    ]);

    let mut files = vec![OutputFile {
        path: format!("src/{name}.{ext}"),
        contents: lines.join("\n"),
    }];
    if !css.is_empty() {
        files.push(OutputFile {
            path: format!("src/{name}.css"),
            contents: css,
        });
    }
    Ok(ExportResult {
        target: "react".into(),
        files,
    })
}

pub fn print_jsx(node: &OutputNode, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    let el = match node {
        OutputNode::Text(text) => return format!("{pad}{}", jsx_text(text)),
        OutputNode::Element(el) => el,
    };

    let attrs: String = el
        .attrs
        .iter()
        .map(|(name, value)| format!(" {}={}", jsx_attr_name(name), jsx_attr_value(value)))
        .collect();
    let open = format!("<{}{attrs}", el.tag);
    match el.children.as_slice() {
        [] => format!("{pad}{open} />"),
        _ if VOID_ELEMENTS.contains(&el.tag) => format!("{pad}{open} />"),
        [OutputNode::Text(text)] => format!("{pad}{open}>{}</{}>", jsx_text(text), el.tag),
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
