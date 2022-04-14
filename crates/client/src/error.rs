use axum::{
    body::Body,
    http::{Response, StatusCode},
    response::IntoResponse,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerError {
    #[error("Error {0}")]
    Unexpected(#[from] anyhow::Error),
    #[error("Tera error")]
    TeraError(#[from] tera::Error),
    #[error("Reqwest error")]
    ReqwestError(#[from] reqwest::Error),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response<axum::body::BoxBody> {
        tracing::error!(?self, "Internal error");
        let body = Body::from("Something went wrong");
        let boxed_body = axum::body::boxed(body);

        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(boxed_body)
            .unwrap()
    }
}
