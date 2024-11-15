use commons::graph;
use failure::{bail, Error, Fallible, SyncFailure};
use reqwest::Method;
use std::time::Duration;

/// Return a request builder with base URL and parameters set.
fn new_request(
    method: reqwest::Method,
    url: reqwest::Url,
    req_timeout: Duration,
) -> Fallible<reqwest::RequestBuilder> {
    let client = reqwest::ClientBuilder::new().timeout(req_timeout).build()?;
    let builder = client.request(method, url);
    Ok(builder)
}

/// Fetch the graph from the fcos-graph-builder instance with the query specified.
pub(crate) async fn fetch_graph_from_gb(
    upstream_base: reqwest::Url,
    stream: String,
    basearch: String,
    oci: bool,
    req_timeout: Duration,
) -> Result<graph::Graph, Error> {
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
        oci: Some(oci),
    };
    // Cannot use `?` directly here otherwise will produce the error:
    //   the trait `std::marker::Sync` is not implemented for `(dyn std::error::Error + std::marker::Send + 'static)`
    // Reference: https://github.com/rust-lang-nursery/failure/issues/284
    let query_str = serde_qs::to_string(&query).map_err(SyncFailure::new)?;
    let mut target = upstream_base;
    target.set_query(Some(&query_str));
    let req = new_request(Method::GET, target, req_timeout)?;
    let resp = req.send().await?;
    let content = resp.error_for_status()?;
    let json = content.json::<graph::Graph>().await?;
    Ok(json)
}
