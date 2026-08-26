use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use include_dir::Dir;
use mime_guess::from_path;
use std::convert::Infallible;
use std::task::{Context, Poll};
use tower::Service;
use tracing::warn;

use include_dir::include_dir;

/// The edge UI, embedded at compile time.
///
/// `build.rs` guarantees this directory exists, so there is no longer a
/// `#[cfg(test)]` arm substituting an empty [`Dir`]. That arm was two
/// mechanisms for one problem and it hid a third: under it, any test of
/// [`StaticFileServiceEdge`] asserted against an empty directory and could
/// never meaningfully fail, while the *bin* target still needed the real
/// directory to exist — which is why CI carried a `mkdir -p` workaround.
static ASSETS_DIR_EDGE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../frontend_edge/dist");
#[derive(Clone)]
pub struct StaticFileServiceEdge;
impl Service<Request<Body>> for StaticFileServiceEdge {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let path = req.uri().path().trim_start_matches('/');

        let response = if let Some(file) = ASSETS_DIR_EDGE.get_file(path) {
            let mime_type = from_path(path).first_or_octet_stream();
            warn!("{:?}", mime_type);
            Response::builder()
                .header(header::CONTENT_TYPE, mime_type.as_ref())
                .body(Body::from(file.contents().to_vec()))
                .unwrap()
        } else {
            StatusCode::NOT_FOUND.into_response()
        };

        std::future::ready(Ok(response))
    }
}
