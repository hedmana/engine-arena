use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use tokio::sync::broadcast::error::RecvError;

use super::{AppState, api::ApiError};
use crate::arena::{ArenaError, MatchConfig, MatchState};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/matches", post(start))
        .route("/api/matches/current", get(current).delete(cancel))
        .route("/api/matches/{id}/pgn", get(pgn))
        .route("/ws/matches", get(events))
}

async fn start(
    State(state): State<Arc<AppState>>,
    Json(config): Json<MatchConfig>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    match state
        .arena
        .start(config, state.registry.clone(), state.analyzer.clone())
        .await
    {
        Ok(id) => Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id })))),
        Err(ArenaError::Busy) => Err(ApiError(
            StatusCode::CONFLICT,
            "a match is already running".into(),
        )),
        Err(ArenaError::Invalid(message)) => {
            Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, message))
        }
    }
}

async fn current(State(state): State<Arc<AppState>>) -> Result<Json<MatchState>, ApiError> {
    state
        .arena
        .current()
        .map(Json)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no match".into()))
}

async fn cancel(State(state): State<Arc<AppState>>) -> Result<StatusCode, ApiError> {
    if state.arena.cancel() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, "no running match".into()))
    }
}

async fn pgn(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u64>,
) -> Result<Response, ApiError> {
    let pgn = tokio::fs::read(state.arena.pgn_path(id))
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::NOT_FOUND,
                "no games saved for this match".into(),
            )
        })?;
    let disposition = format!("attachment; filename=\"engine-arena-match-{id}.pgn\"");
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-chess-pgn".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        pgn,
    )
        .into_response())
}

async fn events(upgrade: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    upgrade.on_upgrade(move |socket| stream(socket, state))
}

/// Sends the current match state, then every event; resends the state if this client lags.
async fn stream(mut socket: WebSocket, state: Arc<AppState>) {
    let (snapshot, mut events) = state.arena.subscribe();
    if socket.send(Message::Text(snapshot.into())).await.is_err() {
        return;
    }
    loop {
        let json: String = tokio::select! {
            event = events.recv() => match event {
                Ok(json) => json.to_string(),
                Err(RecvError::Lagged(_)) => {
                    let (snapshot, receiver) = state.arena.subscribe();
                    events = receiver;
                    snapshot
                }
                Err(RecvError::Closed) => return,
            },
            message = socket.recv() => match message {
                Some(Ok(_)) => continue,
                _ => return,
            },
        };
        if socket.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }
}
