# Vellum

A website builder with a drag-and-drop page editor and a built-in SVG vector canvas, written in Rust.

Architecture plan: https://claude.ai/code/artifact/9921d2f7-9929-42e9-b5e3-fecbe660cee4

The editor is [Leptos](https://leptos.dev) compiled to WebAssembly; the server is [Axum](https://github.com/tokio-rs/axum).

## Run it

One-time setup:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
```

Then, from the repo root:

```sh
cargo dev
```

This builds the editor and serves it at http://127.0.0.1:8080. Your page is saved to `project/document.json`
(change the folder with `cargo dev -- --dir path/to/site`).

| Command | What it does |
| --- | --- |
| `cargo dev` | Build the editor and serve it against a local project folder |
| `cargo run -p vellum-cli -- serve` | Serve an already built editor (for containers) |
| `cargo run -p vellum-cli -- export --target react` | Export `project/document.json` as code into `export/` (`--list` shows targets) |
| `cargo test --workspace` | Run the unit tests |
| `trunk serve` (in `apps/editor`) | Hot-reload the editor while working on it; proxies `/api` to a running `vellum serve` |

## What's here

**Page editor**
- Drag blocks (heading, text, button, image, spacer, vector graphic) from the palette onto the page, or click to append.
- Drag blocks on the page to reorder; select one to edit its content, alignment, colors and padding.
- **Pages:** add, rename, reorder and delete pages, each with its own path (`/about`). A **Navigation** block links every page, and buttons can link to a page by its path.
- **Breakpoints:** switch the canvas to Tablet (900px and narrower) or Mobile (600px and narrower) to preview it, and set per-breakpoint overrides: hide a block, change alignment, padding or type, flip a row to a column, or change a grid's columns. Exports turn them into media queries.
- Undo/redo (Ctrl+Z, Ctrl+Shift+Z) and automatic saving.

**Vector canvas**
- Tools: select (V), pen (P), rectangle (R), ellipse (E), line (L).
- Pen: click for corners, click-and-drag for bezier curves, click the first point to close, Enter or double-click to leave the path open.
- Select a shape to move it, restyle its fill and stroke, or delete it.
- **Add to page** drops the drawing into the page as a vector block; **Edit in vector canvas** on that block reopens it. **Export SVG** downloads it.

## Layout

```
apps/
  editor/     Leptos (WASM): page editor, vector canvas, inspector
  server/     Axum: serves the editor, loads and saves the project document
crates/
  schema/     The site document types (serde) shared by everything
  core/       Editing operations and undo/redo, testable without a browser
  vector/     Path data, shape geometry and SVG output
  export-core/   Exporter trait, registry, shared markup and CSS generation
  export-html/   Static HTML + CSS target
  export-react/  React (JSX/TSX) target
  cli/        The `vellum` command (`dev`, `serve`, `export`)
```

New export targets are new `export-*` crates; see `crates/export-core/README.md`.
