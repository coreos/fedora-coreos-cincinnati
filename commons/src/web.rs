use crate::graph::GraphScope;
use actix_cors::CorsFactory;
use failure::{bail, ensure, err_msg};
use serde::Deserialize;
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

/// Mandatory parameters for graph querying.
#[derive(Deserialize)]
pub struct GraphQuery {
    basearch: Option<String>,
    stream: Option<String>,
}

impl GraphQuery {
    /// Validate input query parameters into a valid graph scope.
    pub fn validate_scope(
        self,
        scope_allowlist: &Option<HashSet<GraphScope>>,
    ) -> Result<GraphScope, failure::Error> {
        let basearch = self.basearch.ok_or_else(|| err_msg("missing basearch"))?;
        ensure!(!basearch.is_empty(), "empty basearch");

        let stream = self.stream.ok_or_else(|| err_msg("missing stream"))?;
        ensure!(!stream.is_empty(), "empty stream");

        let scope = GraphScope { basearch, stream };

        // Optionally filter out scope according to given allowlist, if any.
        if let Some(allowlist) = scope_allowlist {
            if !allowlist.contains(&scope) {
                bail!(
                    "scope not allowed: basearch='{}', stream='{}'",
                    scope.basearch,
                    scope.stream
                );
            }
        }

        Ok(scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_scope() {
        {
            let query = GraphQuery {
                basearch: None,
                stream: None,
            };
            let r = query.validate_scope(&None);
            assert!(r.is_err());
        }
        {
            let query = GraphQuery {
                basearch: Some("test_empty".to_string()),
                stream: Some("".to_string()),
            };
            let r = query.validate_scope(&None);
            assert!(r.is_err());
        }
        {
            let query = GraphQuery {
                basearch: Some("x86_64".to_string()),
                stream: Some("stable".to_string()),
            };
            let r = query.validate_scope(&None);
            assert!(r.is_ok());
        }
        {
            let query = GraphQuery {
                basearch: Some("x86_64".to_string()),
                stream: Some("stable".to_string()),
            };
            let filter_none_allowed = Some(HashSet::new());
            let r = query.validate_scope(&filter_none_allowed);
            assert!(r.is_err());
        }
        {
            let query = GraphQuery {
                basearch: Some("x86_64".to_string()),
                stream: Some("stable".to_string()),
            };
            let allowed_scope = GraphScope {
                basearch: "x86_64".to_string(),
                stream: "stable".to_string(),
            };
            let filter = Some(maplit::hashset! {allowed_scope});
            let r = query.validate_scope(&filter);
            assert!(r.is_ok());
        }
    }
}
