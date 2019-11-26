//! Metrics endpoint.

use crate::AppState;
use actix_web::{HttpRequest, HttpResponse};
use futures::future;
use futures::prelude::*;
use prometheus;

/// Serve metrics requests (Prometheus textual format).
pub(crate) fn serve_metrics(
    _req: HttpRequest<AppState>,
) -> Box<dyn Future<Item = HttpResponse, Error = failure::Error>> {
    use prometheus::Encoder;

    let resp = future::ok(prometheus::default_registry().gather())
        .and_then(|metrics| {
            let tenc = prometheus::TextEncoder::new();
            let mut buf = vec![];
            tenc.encode(&metrics, &mut buf).and(Ok(buf))
        })
        .from_err()
        .map(|content| HttpResponse::Ok().body(content));
    Box::new(resp)
}
