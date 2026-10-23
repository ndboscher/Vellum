use serde::Deserialize;

use vellum_export_core::lower::{lower_site, print_css, OutputNode, VOID_ELEMENTS};
use vellum_export_core::{
    parse_options, ExportError, ExportOptions, ExportResult, Exporter, OutputFile,
};
use vellum_schema::{Document, Page};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct HtmlOptions {
    /// Put the CSS in a `<style>` tag instead of a separate `styles.css`.
    pub inline_css: bool,
}

pub struct HtmlExporter;

impl Exporter for HtmlExporter {
    fn id(&self) -> &'static str {
        "html"
    }

    fn label(&self) -> &'static str {
        "HTML + CSS"
    }

    fn export(&self, doc: &Document, options: &ExportOptions) -> Result<ExportResult, ExportError> {
        Ok(export_html(doc, &parse_options(self.id(), options)?))
    }
}

/// Writes one HTML file per page, named by its path (`index.html`, `about.html`), sharing one
/// stylesheet.
pub fn export_html(doc: &Document, options: &HtmlOptions) -> ExportResult {
    let file_name = |page: &Page| format!("{}.html", page.file_stem());
    let site = lower_site(doc, &file_name);
    let css = print_css(&site.rules);

    let mut files: Vec<_> = site
        .pages
        .into_iter()
        .map(|page| {
            let title = if page.path == "/" {
                doc.title.clone()
            } else {
                format!("{} · {}", page.name, doc.title)
            };
            let mut head = vec![
                r#"<meta charset="utf-8">"#.to_owned(),
                r#"<meta name="viewport" content="width=device-width, initial-scale=1">"#
                    .to_owned(),
                format!("<title>{}</title>", escape_text(&title)),
            ];
            if !options.inline_css {
                head.push(r#"<link rel="stylesheet" href="styles.css">"#.to_owned());
            } else if !css.is_empty() {
                head.push(format!("<style>\n{}\n</style>", indent(css.trim_end(), 1)));
            }

            let mut lines = vec![
                "<!doctype html>".to_owned(),
                r#"<html lang="en">"#.to_owned(),
                "<head>".to_owned(),
                indent(&head.join("\n"), 1),
                "</head>".to_owned(),
                "<body>".to_owned(),
            ];
            lines.push(print_html(&OutputNode::Element(page.root), 1));
            lines.extend(["</body>".to_owned(), "</html>".to_owned(), String::new()]);
            OutputFile {
                path: format!("{}.html", page.file_stem),
                contents: lines.join("\n"),
            }
        })
        .collect();
    if !options.inline_css {
        files.push(OutputFile {
            path: "styles.css".into(),
            contents: css,
        });
    }
    ExportResult {
        target: "html".into(),
        files,
    }
}

pub fn print_html(node: &OutputNode, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    let el = match node {
        OutputNode::Text(text) => return format!("{pad}{}", escape_text(text)),
        OutputNode::Element(el) => el,
        // HTML has no components: print each instance's own markup.
        OutputNode::Instance(instance) => {
            return print_html(&OutputNode::Element(instance.expanded.clone()), depth)
        }
        // Props only appear in component templates, which this target doesn't print.
        OutputNode::Prop(_) => return String::new(),
    };

    let attrs: String = el
        .attrs
        .iter()
        .map(|(name, value)| format!(r#" {name}="{}""#, escape_attr(value)))
        .collect();
    let open = format!("<{}{attrs}>", el.tag);
    if VOID_ELEMENTS.contains(&el.tag) {
        return format!("{pad}{open}");
    }
    match el.children.as_slice() {
        [] => format!("{pad}{open}</{}>", el.tag),
        [OutputNode::Text(text)] => format!("{pad}{open}{}</{}>", escape_text(text), el.tag),
        children => {
            let mut lines = vec![format!("{pad}{open}")];
            lines.extend(children.iter().map(|child| print_html(child, depth + 1)));
            lines.push(format!("{pad}</{}>", el.tag));
            lines.join("\n")
        }
    }
}

pub fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn escape_attr(value: &str) -> String {
    escape_text(value).replace('"', "&quot;")
}

fn indent(text: &str, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{pad}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
