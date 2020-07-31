use actix::prelude::*;
use commons::graph;
use failure::{bail, Error, Fallible, SyncFailure};
use reqwest::Method;
use std::time::Duration;

/// Default timeout for HTTP requests (30 minutes).
const DEFAULT_HTTP_REQ_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Default address of fcos-graph-builder, which is running in the same pod.
const DEFAULT_GB_ADDR: &str = "http://127.0.0.1:8080/v1/graph";

/// Return a request builder with base URL and parameters set.
fn new_request(method: reqwest::Method, url: reqwest::Url) -> Fallible<reqwest::RequestBuilder> {
    let client = reqwest::ClientBuilder::new()
        .timeout(DEFAULT_HTTP_REQ_TIMEOUT)
        .build()?;
    let builder = client.request(method, url);
    Ok(builder)
}

/// Fetch the graph from the fcos-graph-builder instance with the query specified.
pub(crate) fn fetch_graph_from_gb(
    stream: String,
    basearch: String,
) -> impl Future<Output = Result<graph::Graph, Error>> {
    async move {
        if stream.trim().is_empty() {
            bail!("unexpected missing stream");
        }
        if basearch.trim().is_empty() {
            bail!("unexpected missing basearch");
        }
        let query = crate::GraphQuery {
            stream: Some(stream),
            basearch: Some(basearch),
            rollout_wariness: None,
            node_uuid: None,
        };
        // Cannot use `?` directly here otherwise will produce the error:
        //   the trait `std::marker::Sync` is not implemented for `(dyn std::error::Error + std::marker::Send + 'static)`
        // Reference: https://github.com/rust-lang-nursery/failure/issues/284
        let query_str = serde_qs::to_string(&query).map_err(SyncFailure::new)?;
        let mut target = reqwest::Url::parse(DEFAULT_GB_ADDR)?;
        target.set_query(Some(&query_str));
        let req = new_request(Method::GET, target)?;
        let resp = req.send().await?;
        let content = resp.error_for_status()?;
        let json = content.json::<graph::Graph>().await?;
        Ok(json)
    }
}
