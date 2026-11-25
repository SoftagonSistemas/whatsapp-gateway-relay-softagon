use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "console/dist"]
struct ConsoleAssets;

pub async fn redirect_to_console() -> Redirect {
    Redirect::permanent("/console/")
}

pub async fn console_index() -> Response {
    serve_asset("index.html")
}

pub async fn console_fallback(Path(path): Path<String>) -> Response {
    let clean = path.trim_start_matches('/');
    if !clean.is_empty() && ConsoleAssets::get(clean).is_some() {
        return serve_asset(clean);
    }
    serve_asset("index.html")
}

fn serve_asset(path: &str) -> Response {
    let Some(file) = ConsoleAssets::get(path) else {
        return (StatusCode::NOT_FOUND, "console asset not found").into_response();
    };

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    let content_type = HeaderValue::from_str(mime.as_ref())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        file.data.into_owned(),
    )
        .into_response()
}
