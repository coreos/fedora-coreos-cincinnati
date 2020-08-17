use actix_cors::CorsFactory;

/// Provide a CORS middleware allowing given origins.
pub fn build_cors_middleware(allowed_origins: &[impl AsRef<str>]) -> CorsFactory {
    let mut builder = actix_cors::Cors::new();
    for origin in allowed_origins {
        builder = builder.allowed_origin(origin.as_ref());
    }
    builder.finish()
}
