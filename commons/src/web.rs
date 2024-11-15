use crate::graph::GraphScope;
use actix_cors::CorsFactory;
use failure::{bail, ensure, err_msg};
use std::collections::HashSet;

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

/// Validate input query parameters into a valid graph scope.
pub fn validate_scope(
    basearch: Option<String>,
    stream: Option<String>,
    oci: Option<bool>,
    scope_allowlist: &Option<HashSet<GraphScope>>,
) -> Result<GraphScope, failure::Error> {
    let basearch = basearch.ok_or_else(|| err_msg("missing basearch"))?;
    ensure!(!basearch.is_empty(), "empty basearch");

    let stream = stream.ok_or_else(|| err_msg("missing stream"))?;
    ensure!(!stream.is_empty(), "empty stream");

    let oci = oci.unwrap_or_default();

    let scope = GraphScope {
        basearch,
        stream,
        oci,
    };

    // Optionally filter out scope according to given allowlist, if any.
    if let Some(allowlist) = scope_allowlist {
        if !allowlist.contains(&scope) {
            bail!(
                "scope not allowed: basearch='{}', stream='{}', oci='{}'",
                scope.basearch,
                scope.stream,
                scope.oci,
            );
        }
    }

    Ok(scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_scope() {
        {
            let r = validate_scope(None, None, None, &None);
            assert!(r.is_err());
        }
        {
            let basearch = Some("test_empty".to_string());
            let stream = Some("".to_string());
            let oci = None;
            let r = validate_scope(basearch, stream, oci, &None);
            assert!(r.is_err());
        }
        {
            let basearch = Some("x86_64".to_string());
            let stream = Some("stable".to_string());
            let oci = Some(false);
            let r = validate_scope(basearch, stream, oci, &None);
            assert!(r.is_ok());
        }
        {
            let basearch = Some("x86_64".to_string());
            let stream = Some("stable".to_string());
            let filter_none_allowed = Some(HashSet::new());
            let r = validate_scope(basearch, stream, None, &filter_none_allowed);
            assert!(r.is_err());
        }
        {
            let basearch = Some("x86_64".to_string());
            let stream = Some("stable".to_string());
            let allowed_scope = GraphScope {
                basearch: "x86_64".to_string(),
                stream: "stable".to_string(),
                oci: false,
            };
            let filter = Some(maplit::hashset! {allowed_scope});
            let r = validate_scope(basearch, stream, None, &filter);
            assert!(r.is_ok());
        }
    }
}
