//! The four REST endpoints. Everything else happens over the socket.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::protocol::*;
use crate::state::{AppState, db_path};
use crate::{db, replay};

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
        Some((player_id, player_token)) => Json(JoinSessionResponse {
            player_id,
            player_token,
        })
        .into_response(),
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

    // Prefer the database: it is the durable record and it outlives the
    // session's in-memory copy. Fall back to the actor when there is no
    // database, so the export still works with persistence unavailable.
    let body = db::open(&db_path())
        .and_then(|conn| db::read_trades(&conn, &handle.code))
        .ok()
        .filter(|rows| !rows.is_empty())
        .map(|rows| {
            let mut out = String::from("seq,ts,price,buyer,seller,aggressor,self_trade\n");
            for t in rows {
                out.push_str(&format!(
                    "{},{},{},{},{},{},{}\n",
                    t.seq,
                    t.ts,
                    t.price,
                    csv_escape(&t.buyer),
                    csv_escape(&t.seller),
                    t.aggressor,
                    t.self_trade
                ));
            }
            out
        });

    let body = match body {
        Some(csv) => csv,
        None => match handle.export().await {
            Some(csv) => csv,
            None => return (StatusCode::GONE, "session ended").into_response(),
        },
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

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// GET /api/sessions/:code/verify?hostToken=
///
/// Replays the session from its command log and compares the result against
/// live state. This is the payoff for keeping the engine pure: if these ever
/// disagree, something non-deterministic has got into it.
pub async fn verify(
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

    let Some(live) = handle.fingerprint().await else {
        return (StatusCode::GONE, "session ended").into_response();
    };

    let replayed = match db::open(&db_path()).and_then(|conn| replay::replay(&conn, &handle.code)) {
        Ok(market) => market.fingerprint(),
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("replay failed: {e}"),
            )
                .into_response();
        }
    };

    Json(serde_json::json!({
        "matches": live == replayed,
        "live": live,
        "replayed": replayed,
    }))
    .into_response()
}
