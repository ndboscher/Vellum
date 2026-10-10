//! The Vellum server: serves the built editor and saves the project document to a folder.
//!
//! Local mode keeps one `document.json` in the project folder. Cloud storage (Postgres + S3)
//! will slot in behind the same two routes.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use tower_http::services::{ServeDir, ServeFile};
use vellum_schema::Document;

const DOCUMENT_FILE: &str = "document.json";

pub struct Config {
    /// Folder holding the project's `document.json`. Created if missing.
    pub project_dir: PathBuf,
    /// Folder holding the built editor (`index.html`, wasm, css).
    pub editor_dir: PathBuf,
    pub addr: SocketAddr,
}

#[derive(Clone)]
struct AppState {
    project_dir: Arc<PathBuf>,
}

pub fn router(project_dir: PathBuf, editor_dir: &Path) -> Router {
    let state = AppState {
        project_dir: Arc::new(project_dir),
    };
    let editor = ServeDir::new(editor_dir).fallback(ServeFile::new(editor_dir.join("index.html")));
    Router::new()
        .route("/api/document", get(load_document).put(save_document))
        .with_state(state)
        .fallback_service(editor)
}

pub async fn serve(config: Config) -> std::io::Result<()> {
    tokio::fs::create_dir_all(&config.project_dir).await?;
    let listener = tokio::net::TcpListener::bind(config.addr).await?;
    axum::serve(listener, router(config.project_dir, &config.editor_dir)).await
}

async fn load_document(State(state): State<AppState>) -> Response {
    let path = state.project_dir.join(DOCUMENT_FILE);
    match tokio::fs::read(&path).await {
        Ok(bytes) => match serde_json::from_slice::<Document>(&bytes) {
            Ok(doc) => Json(doc).into_response(),
            Err(e) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("{}: {e}", path.display()),
            )
                .into_response(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn save_document(State(state): State<AppState>, Json(doc): Json<Document>) -> Response {
    let json = match serde_json::to_vec_pretty(&doc) {
        Ok(json) => json,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    // Write then rename, so a crash mid-save never leaves a truncated document.
    let tmp = state.project_dir.join(format!("{DOCUMENT_FILE}.tmp"));
    let result = async {
        tokio::fs::create_dir_all(state.project_dir.as_path()).await?;
        tokio::fs::write(&tmp, json).await?;
        tokio::fs::rename(&tmp, state.project_dir.join(DOCUMENT_FILE)).await
    }
    .await;
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vellum-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn saves_and_loads_the_document() {
        let project = temp_dir("project");
        let editor = temp_dir("editor");
        std::fs::write(editor.join("index.html"), "<!doctype html>editor").unwrap();
        let app = router(project.clone(), &editor);

        let missing = app
            .clone()
            .oneshot(Request::get("/api/document").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let doc = Document {
            title: "Saved".into(),
            ..Document::default()
        };
        let put = Request::put("/api/document")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&doc).unwrap()))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(put).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
        assert!(project.join(DOCUMENT_FILE).exists());

        let got = app
            .clone()
            .oneshot(Request::get("/api/document").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(got.status(), StatusCode::OK);
        let body = got.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(serde_json::from_slice::<Document>(&body).unwrap(), doc);

        let page = app
            .oneshot(Request::get("/some/route").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = page.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"<!doctype html>editor");
    }

    #[tokio::test]
    async fn rejects_a_malformed_document() {
        let app = router(temp_dir("bad"), &temp_dir("bad-editor"));
        let put = Request::put("/api/document")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"nodes": 3}"#))
            .unwrap();
        assert!(app.oneshot(put).await.unwrap().status().is_client_error());
    }
}
