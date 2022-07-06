//! Metrics endpoint.

use actix_web::HttpResponse;

/// Serve metrics requests (Prometheus textual format).
pub async fn serve_metrics() -> HttpResponse {
    use prometheus::Encoder;

    let metrics = prometheus::default_registry().gather();
    let txt_enc = prometheus::TextEncoder::new();
    let mut buf = vec![];
    match txt_enc.encode(&metrics, &mut buf) {
        Err(_) => HttpResponse::InternalServerError().finish(),
        Ok(content) => HttpResponse::Ok().body(content),
    }
}
