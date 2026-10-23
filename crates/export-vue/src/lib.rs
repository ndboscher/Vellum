use serde::Deserialize;

use vellum_export_core::lower::{lower_site, pascal_case, print_css, OutputNode, VOID_ELEMENTS};
use vellum_export_core::{
    parse_options, ExportError, ExportOptions, ExportResult, Exporter, OutputFile,
};
use vellum_schema::{Document, Page};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct VueOptions {
    /// Name of the root component, which picks a page by URL path.
    pub component_name: String,
    /// Use `<script setup lang="ts">`.
    pub typescript: bool,
}

impl Default for VueOptions {
    fn default() -> Self {
        Self {
            component_name: "App".into(),
            typescript: false,
        }
    }
}

pub struct VueExporter;

impl Exporter for VueExporter {
    fn id(&self) -> &'static str {
        "vue"
    }

    fn label(&self) -> &'static str {
        "Vue"
    }

    fn export(&self, doc: &Document, options: &ExportOptions) -> Result<ExportResult, ExportError> {
        export_vue(doc, &parse_options(self.id(), options)?)
    }
}

/// Writes Vue 3 single-file components: one per page under `src/pages/`, one per component
/// under `src/components/` with its props, a root that renders the page for the current URL
/// path, and one shared stylesheet.
pub fn export_vue(doc: &Document, options: &VueOptions) -> Result<ExportResult, ExportError> {
    let name = &options.component_name;
    let valid_name = name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_name {
        return Err(ExportError::InvalidOptions {
            target: "vue".into(),
            message: format!("componentName must be a PascalCase identifier, got \"{name}\""),
        });
    }
    let script = if options.typescript {
        r#"<script setup lang="ts">"#
    } else {
        "<script setup>"
    };
    let site_path = |page: &Page| page.path.clone();
    let site = lower_site(doc, &site_path);
    let css = print_css(&site.rules);

    let mut used = vec![name.clone()];
    used.extend(site.components.iter().map(|c| c.name.clone()));
    let pages: Vec<String> = site
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
    let mut app = vec![script.to_owned()];
    if !css.is_empty() {
        app.push("import \"./styles.css\";".to_owned());
    }
    for (page, component) in site.pages.iter().zip(&pages) {
        app.push(format!(
            "import {component} from \"./pages/{component}.vue\";"
        ));
        let root = OutputNode::Element(page.root.clone());
        files.push(OutputFile {
            path: format!("src/pages/{component}.vue"),
            contents: sfc(script, imports(&root, "../components"), &root),
        });
    }
    for component in &site.components {
        let root = OutputNode::Element(component.root.clone());
        let mut script_lines = imports(&root, ".");
        if !component.props.is_empty() {
            script_lines.push("defineProps({".to_owned());
            for prop in &component.props {
                script_lines.push(format!(
                    "  {}: {{ type: String, default: {} }},",
                    prop.name,
                    js_string(&prop.default)
                ));
            }
            script_lines.push("});".to_owned());
        }
        files.push(OutputFile {
            path: format!("src/components/{}.vue", component.name),
            contents: sfc(script, script_lines, &root),
        });
    }
    app.push(String::new());
    app.push("const routes = {".to_owned());
    for (page, component) in site.pages.iter().zip(&pages) {
        app.push(format!("  {}: {component},", js_string(&page.path)));
    }
    app.extend([
        "};".to_owned(),
        "const path = window.location.pathname.replace(/\\/+$/, \"\") || \"/\";".to_owned(),
        format!("const page = routes[path] ?? {};", pages[0]),
        "</script>".to_owned(),
        String::new(),
        "<template>".to_owned(),
        "  <component :is=\"page\" />".to_owned(),
        "</template>".to_owned(),
        String::new(),
    ]);
    files.insert(
        0,
        OutputFile {
            path: format!("src/{name}.vue"),
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
        target: "vue".into(),
        files,
    })
}

/// A single-file component: a `<script setup>` block (left out when empty) and a template.
fn sfc(script: &str, script_lines: Vec<String>, root: &OutputNode) -> String {
    let mut lines = Vec::new();
    let script_lines: Vec<_> = script_lines.into_iter().filter(|l| !l.is_empty()).collect();
    if !script_lines.is_empty() {
        lines.push(script.to_owned());
        lines.extend(script_lines);
        lines.push("</script>".to_owned());
        lines.push(String::new());
    }
    lines.extend([
        "<template>".to_owned(),
        print_template(root, 1),
        "</template>".to_owned(),
        String::new(),
    ]);
    lines.join("\n")
}

/// Import lines for every component used in `root`.
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
    names
        .iter()
        .map(|n| format!("import {n} from \"{dir}/{n}.vue\";"))
        .collect()
}

pub fn print_template(node: &OutputNode, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    let el = match node {
        OutputNode::Text(text) => return format!("{pad}{}", escape_text(text)),
        OutputNode::Prop(prop) => return format!("{pad}{{{{ {prop} }}}}"),
        OutputNode::Instance(instance) => {
            let props: String = instance
                .props
                .iter()
                .map(|(name, value)| format!(r#" {name}="{}""#, escape_attr(value)))
                .collect();
            return format!("{pad}<{}{props} />", instance.component);
        }
        OutputNode::Element(el) => el,
    };

    let mut attrs: String = el
        .attrs
        .iter()
        .map(|(name, value)| format!(r#" {name}="{}""#, escape_attr(value)))
        .collect();
    for (name, prop) in &el.prop_attrs {
        attrs.push_str(&format!(r#" :{name}="{prop}""#));
    }
    let open = format!("<{}{attrs}", el.tag);
    match el.children.as_slice() {
        _ if VOID_ELEMENTS.contains(&el.tag) => format!("{pad}{open} />"),
        [] => format!("{pad}{open}></{}>", el.tag),
        [child @ (OutputNode::Text(_) | OutputNode::Prop(_))] => {
            format!("{pad}{open}>{}</{}>", print_template(child, 0), el.tag)
        }
        children => {
            let mut lines = vec![format!("{pad}{open}>")];
            lines.extend(
                children
                    .iter()
                    .map(|child| print_template(child, depth + 1)),
            );
            lines.push(format!("{pad}</{}>", el.tag));
            lines.join("\n")
        }
    }
}

/// HTML-escapes text, and escapes `{` so text can't open a `{{ }}` interpolation.
fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('{', "&#123;")
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A JSON string literal is a valid JS string literal.
fn js_string(value: &str) -> String {
    serde_json::to_string(value).expect("strings always serialize")
}
