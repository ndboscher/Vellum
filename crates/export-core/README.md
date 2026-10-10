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
let result = registry.export("react", &document, &options)?; // src/Landing.jsx, src/Landing.css
```

From the command line: `vellum export --target react --set componentName=Landing --out app/`.

## Pipeline

1. **`Document`** (`crates/schema`) is the page the editor produces.
2. **`lower_document`** (`src/lower.rs`) converts it once into a target-neutral element tree plus CSS rules. Shared decisions live here: which tag each block becomes, class names, base styles copied from the editor canvas so exports look like what was built, vector drawings as inline SVG (path data from `vellum-vector`), and sanitizing (script URLs become `#`, style values that could break out of a rule are dropped).
3. **Exporters** print that tree in their own syntax and return `OutputFile`s.

| Target | Crate | Options | Files |
| --- | --- | --- | --- |
| `html` | `export-html` | `inlineCss` | `index.html`, `styles.css` (omitted when inlined) |
| `react` | `export-react` | `componentName` (default `Page`), `typescript` | `src/<Name>.jsx` or `.tsx`, `src/<Name>.css` |

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
        let page = lower_document(doc);
        // print page.root as a <template>, print_css(&page.rules) as a <style> block
        todo!()
    }
}
```

Markup targets (Vue, Svelte, Astro) only need a printer for the element tree. Non-markup targets can walk `Document` directly.

## Tests

`crates/export-core/fixtures/landing.json` covers every block and shape type. Exporter output is checked with `insta` snapshots in each target crate; after an intended output change, run `cargo insta review` (or `INSTA_UPDATE=always cargo test`).
