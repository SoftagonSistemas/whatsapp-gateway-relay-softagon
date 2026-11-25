use std::sync::OnceLock;

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};

pub const OPENAPI_SPEC_YAML: &str = include_str!("../../openapi/openapi.yaml");

static OPENAPI_SPEC_JSON: OnceLock<String> = OnceLock::new();

pub fn openapi_spec_json() -> &'static str {
    OPENAPI_SPEC_JSON.get_or_init(|| {
        let value: serde_json::Value = serde_yaml_ng::from_str(OPENAPI_SPEC_YAML)
            .expect("embedded openapi/openapi.yaml must be valid YAML");
        serde_json::to_string_pretty(&value).expect("OpenAPI spec must serialize to JSON")
    })
}

const SCALAR_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Softagon Relay — API Reference</title>
  </head>
  <body>
    <script
      id="api-reference"
      data-url="/openapi.yaml"
    ></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </body>
</html>
"#;

pub async fn openapi_yaml() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/yaml; charset=utf-8"),
        )],
        OPENAPI_SPEC_YAML,
    )
        .into_response()
}

pub async fn openapi_json() -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        openapi_spec_json(),
    )
        .into_response()
}

pub async fn scalar_docs() -> Html<&'static str> {
    Html(SCALAR_HTML)
}
