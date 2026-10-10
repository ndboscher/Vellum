//! `vellum` command-line tool.

use std::{net::SocketAddr, path::PathBuf, process::Command};

use clap::{Parser, Subcommand};
use vellum_export_core::{ExportOptions, ExporterRegistry};

#[derive(Parser)]
#[command(name = "vellum", version, about = "Build websites and vector graphics")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build the editor and serve it against a local project folder.
    Dev {
        /// Project folder; the document is saved as `document.json` inside it.
        #[arg(long, default_value = "project")]
        dir: PathBuf,
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Skip rebuilding the editor (use the existing build).
        #[arg(long)]
        no_build: bool,
    },
    /// Serve an already built editor (for containers and production).
    Serve {
        #[arg(long, default_value = "project")]
        dir: PathBuf,
        #[arg(long, default_value = "apps/editor/dist")]
        editor: PathBuf,
        #[arg(long, default_value = "0.0.0.0:8080")]
        addr: SocketAddr,
    },
    /// Export the project's page as code.
    Export {
        #[arg(long, default_value = "project")]
        dir: PathBuf,
        /// Export target; run with `--list` to see them.
        #[arg(long, default_value = "html")]
        target: String,
        /// Folder to write the generated files into.
        #[arg(long, default_value = "export")]
        out: PathBuf,
        /// Target option as `key=value`, e.g. `--set componentName=Landing`. Repeatable.
        #[arg(long = "set", value_name = "KEY=VALUE")]
        options: Vec<String>,
        /// List the available targets and exit.
        #[arg(long)]
        list: bool,
    },
}

/// Every export target the CLI knows about.
fn registry() -> ExporterRegistry {
    let mut registry = ExporterRegistry::new();
    registry
        .register(vellum_export_html::HtmlExporter)
        .and_then(|r| r.register(vellum_export_react::ReactExporter))
        .expect("built-in export targets have unique ids");
    registry
}

fn export(
    dir: PathBuf,
    target: String,
    out: PathBuf,
    raw_options: Vec<String>,
) -> Result<(), String> {
    let mut options = ExportOptions::new();
    for pair in raw_options {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("expected key=value, got \"{pair}\""))?;
        // `true`, `3` and `"x"` parse as JSON; anything else is taken as a string.
        let value = serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.to_owned()));
        options.insert(key.to_owned(), value);
    }

    let path = dir.join("document.json");
    let source = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: vellum_schema::Document =
        serde_json::from_str(&source).map_err(|e| format!("{}: {e}", path.display()))?;
    let result = registry()
        .export(&target, &doc, &options)
        .map_err(|e| e.to_string())?;

    for file in &result.files {
        let path = out.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, &file.contents).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// The editor crate, resolved from this crate's location so `cargo dev` works from any directory.
fn editor_crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/editor")
}

fn build_editor(editor: &PathBuf) -> Result<(), String> {
    println!("Building the editor with trunk…");
    let status = Command::new("trunk")
        .arg("build")
        .current_dir(editor)
        .status()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "`trunk` was not found. Install it once with:\n  \
             rustup target add wasm32-unknown-unknown\n  cargo install trunk --locked"
                    .to_string()
            } else {
                format!("could not run trunk: {e}")
            }
        })?;
    if status.success() {
        Ok(())
    } else {
        Err("the editor build failed (see trunk output above)".into())
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = match cli.command {
        Cmd::Dev {
            dir,
            port,
            no_build,
        } => {
            let editor = editor_crate_dir();
            if !no_build {
                if let Err(e) = build_editor(&editor) {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
            vellum_server::Config {
                project_dir: dir,
                editor_dir: editor.join("dist"),
                addr: SocketAddr::from(([127, 0, 0, 1], port)),
            }
        }
        Cmd::Export { list: true, .. } => {
            for target in registry().list() {
                println!("{:<8}{}", target.id, target.label);
            }
            return;
        }
        Cmd::Export {
            dir,
            target,
            out,
            options,
            ..
        } => {
            if let Err(e) = export(dir, target, out, options) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
            return;
        }
        Cmd::Serve { dir, editor, addr } => vellum_server::Config {
            project_dir: dir,
            editor_dir: editor,
            addr,
        },
    };
    println!(
        "Vellum is running at http://{}  (project: {})",
        config.addr,
        config.project_dir.display()
    );
    if let Err(e) = vellum_server::serve(config).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
