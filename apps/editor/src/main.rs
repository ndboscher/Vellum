//! The Vellum editor, compiled to WebAssembly and mounted into the page.

mod app;
mod page;
mod persistence;
mod shapes;
mod store;
mod vector;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
