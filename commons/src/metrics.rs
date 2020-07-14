//! Metrics endpoint.

use actix_web::HttpResponse;

/// Serve metrics requests (Prometheus textual format).
pub async fn serve_metrics() -> Result<HttpResponse, failure::Error> {
    use prometheus::Encoder;

    let content = {
        let metrics = prometheus::default_registry().gather();
        let txt_enc = prometheus::TextEncoder::new();
        let mut buf = vec![];
        txt_enc.encode(&metrics, &mut buf)?;
        buf
    };

    Ok(HttpResponse::Ok().body(content))
}
