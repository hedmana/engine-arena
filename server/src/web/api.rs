use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, State, multipart::MultipartError},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

use super::AppState;
use crate::engines::{EngineEntry, MAX_UPLOAD, RegistryError, Upload};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/engines", get(list).post(create))
        .route(
            "/api/engines/{id}",
            post(replace).patch(update).delete(delete),
        )
        .layer(DefaultBodyLimit::max(MAX_UPLOAD + 64 * 1024))
}

pub(super) struct ApiError(pub(super) StatusCode, pub(super) String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<RegistryError> for ApiError {
    fn from(e: RegistryError) -> Self {
        match e {
            RegistryError::NotFound => Self(StatusCode::NOT_FOUND, e.to_string()),
            RegistryError::Invalid(message) => Self(StatusCode::UNPROCESSABLE_ENTITY, message),
            RegistryError::Io(e) => {
                tracing::error!("engine registry: {e}");
                Self(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
            }
        }
    }
}

impl From<MultipartError> for ApiError {
    fn from(e: MultipartError) -> Self {
        Self(e.status(), e.body_text())
    }
}

async fn list(State(state): State<Arc<AppState>>) -> Result<Json<Vec<EngineEntry>>, ApiError> {
    Ok(Json(
        state.registry.list().await.map_err(RegistryError::Io)?,
    ))
}

async fn create(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<EngineEntry>), ApiError> {
    let upload = read_upload(multipart).await?;
    Ok((
        StatusCode::CREATED,
        Json(state.registry.create(upload).await?),
    ))
}

async fn replace(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<Json<EngineEntry>, ApiError> {
    let upload = read_upload(multipart).await?;
    Ok(Json(state.registry.replace(&id, upload).await?))
}

#[derive(Deserialize)]
struct Patch {
    name: Option<String>,
    options: Option<BTreeMap<String, String>>,
}

async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(patch): Json<Patch>,
) -> Result<Json<EngineEntry>, ApiError> {
    Ok(Json(
        state
            .registry
            .update(&id, patch.name, patch.options)
            .await?,
    ))
}

async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.registry.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Reads a `file` field and an optional `name` field. Only the file name's extension is used.
async fn read_upload(mut multipart: Multipart) -> Result<Upload, ApiError> {
    let (mut name, mut file) = (None, None);
    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("name") => name = Some(field.text().await?),
            Some("file") => {
                let python = field.file_name().is_some_and(|n| n.ends_with(".py"));
                file = Some((python, field.bytes().await?.to_vec()));
            }
            _ => {}
        }
    }
    let (python, bytes) =
        file.ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "missing file field".into()))?;
    Ok(Upload {
        name,
        python,
        bytes,
    })
}
