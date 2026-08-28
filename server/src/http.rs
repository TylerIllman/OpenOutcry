//! The four REST endpoints. Everything else happens over the socket.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::protocol::*;
use crate::state::AppState;

/// POST /api/sessions
pub async fn create_session(
    State(app): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    if req.question.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "question is required").into_response();
    }
    let handle = app.create(req).await;
    Json(CreateSessionResponse {
        code: handle.code.clone(),
        host_token: handle.host_token.clone(),
    })
    .into_response()
}

/// GET /api/sessions/:code — public metadata for the join screen.
pub async fn get_session(
    State(app): State<AppState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    match app.get(&code).await {
        None => (StatusCode::NOT_FOUND, "no such session").into_response(),
        Some(handle) => match handle.meta().await {
            Some(meta) => Json(meta).into_response(),
            None => (StatusCode::GONE, "session ended").into_response(),
        },
    }
}

/// POST /api/sessions/:code/join
pub async fn join_session(
    State(app): State<AppState>,
    Path(code): Path<String>,
    Json(req): Json<JoinSessionRequest>,
) -> impl IntoResponse {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    let Some(handle) = app.get(&code).await else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    match handle.join(name).await {
        Some((player_id, player_token)) => {
            Json(JoinSessionResponse { player_id, player_token }).into_response()
        }
        None => (StatusCode::GONE, "session ended").into_response(),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    pub host_token: String,
}

/// GET /api/sessions/:code/export.csv?hostToken=...
pub async fn export_csv(
    State(app): State<AppState>,
    Path(code): Path<String>,
    Query(q): Query<ExportQuery>,
) -> impl IntoResponse {
    let Some(handle) = app.get(&code).await else {
        return (StatusCode::NOT_FOUND, "no such session").into_response();
    };
    if q.host_token != handle.host_token {
        return (StatusCode::FORBIDDEN, "not the host").into_response();
    }

    let body = match handle.export().await {
        Some(csv) => csv,
        None => return (StatusCode::GONE, "session ended").into_response(),
    };

    (
        StatusCode::OK,
        [
            ("content-type", "text/csv; charset=utf-8"),
            ("content-disposition", "attachment; filename=\"tape.csv\""),
        ],
        body,
    )
        .into_response()
}
