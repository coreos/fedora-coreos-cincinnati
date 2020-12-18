use actix_cors::CorsFactory;

/// Build a CORS middleware.
///
/// By default, this allows all CORS requests from all origins.
/// If an allowlist is provided, only those origins are allowed instead.
pub fn build_cors_middleware(origin_allowlist: &Option<Vec<String>>) -> CorsFactory {
    let mut builder = actix_cors::Cors::new();
    match origin_allowlist {
        Some(allowed) => {
            for origin in allowed {
                builder = builder.allowed_origin(origin.as_ref());
            }
        }
        None => {
            builder = builder.send_wildcard();
        }
    };
    builder.finish()
}
