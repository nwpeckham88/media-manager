use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::api::routes::AppState;

pub async fn api_token_middleware(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if path == "/api/health" || path == "/api/config/branding" {
        return Ok(next.run(req).await);
    }

    let Some(expected_token) = &state.api_token else {
        return Ok(next.run(req).await);
    };

    let provided = bearer_token(&headers);
    if provided.as_deref() != Some(expected_token.as_str()) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get("authorization")?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    Some(token.to_string())
}
