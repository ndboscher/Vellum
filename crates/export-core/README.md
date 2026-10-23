# vellum-export-core

The shared half of every Vellum export target. Targets live in their own crates (`export-html`, `export-react`, later `export-vue`) and plug in through one trait.

```rust
use vellum_export_core::{ExportOptions, ExporterRegistry};

let mut registry = ExporterRegistry::new();
registry
    .register(vellum_export_html::HtmlExporter)?
    .register(vellum_export_react::ReactExporter)?;

let mut options = ExportOptions::new();
options.insert("componentName".into(), "Landing".into());
let result = registry.export("react", &document, &options)?; // src/Landing.jsx, src/pages/*.jsx, src/styles.css
```

From the command line: `vellum export --target react --set componentName=Landing --out app/`.

## Pipeline

1. **`Document`** (`crates/schema`) is the site the editor produces: tokens and one or more pages.
2. **`lower_site`** (`src/lower.rs`) converts it once into a target-neutral element tree per page plus one set of CSS rules shared by every page. The caller passes how it links to a page (`about.html`, `/about`), and links to a site path like `/about#team` are rewritten with it. Breakpoint overrides become `@media (max-width: …)` rules after the base rules, widest first. Component instances become `OutputNode::Instance`s carrying both the component's name with the instance's props and the `expanded` markup; each component's nodes get one set of classes and rules however many times it is placed, and `LoweredSite::components` holds each component's template, where bound fields are `OutputNode::Prop`s and `prop_attrs`. Targets with components (React) print the instance and the template; the rest (HTML) print `expanded`. Shared decisions live here: which tag each block becomes, class names, base styles copied from the editor canvas so exports look like what was built, vector drawings as inline SVG (path data from `vellum-vector`), and sanitizing (script URLs become `#`, style values that could break out of a rule are dropped).
3. **Exporters** print that tree in their own syntax and return `OutputFile`s.

| Target | Crate | Options | Files |
| --- | --- | --- | --- |
| `html` | `export-html` | `inlineCss` | one `<page>.html` per page (`index.html` for `/`), `styles.css` (omitted when inlined) |
| `react` | `export-react` | `componentName` (default `App`), `typescript` | `src/<Name>.jsx` or `.tsx` (picks a page by URL path), `src/pages/<Page>.jsx`, `src/components/<Component>.jsx`, `src/styles.css` |

Options arrive as a JSON map so the CLI, server and editor can pass them through untouched. Each target parses them into a typed struct with `parse_options`, and unknown keys are rejected so typos surface.

When the canvas markup or `apps/editor/style.css` changes, update `BASE_RULES` and the block markup in `lower.rs` to match.

## Adding a target

Make an `export-<name>` crate depending on `vellum-schema` and `vellum-export-core`, implement `Exporter`, and register it in `crates/cli`:

```rust
pub struct VueExporter;

impl Exporter for VueExporter {
    fn id(&self) -> &'static str { "vue" }
    fn label(&self) -> &'static str { "Vue" }
    fn export(&self, doc: &Document, options: &ExportOptions) -> Result<ExportResult, ExportError> {
        let site = lower_site(doc, &|page| page.path.clone());
        // print each page's root as a <template>, print_css(&site.rules) as a stylesheet
        todo!()
    }
}
```

Markup targets (Vue, Svelte, Astro) only need a printer for the element tree. Non-markup targets can walk `Document` directly.

## Tests

`crates/export-core/fixtures/landing.json` covers every block and shape type, and stays a version 2 file so loading old documents keeps working. `fixtures/site.json` is a two-page version 4 site with navigation, page links, breakpoint overrides and a component with props placed twice. Exporter output is checked with `insta` snapshots in each target crate; after an intended output change, run `cargo insta review` (or `INSTA_UPDATE=always cargo test`).
