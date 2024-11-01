use actix::prelude::*;
use actix_web::web::Bytes;
use commons::{graph, metadata};
use failure::{Error, Fallible};
use reqwest::Method;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::time::Duration;

/// Default timeout for HTTP requests (30 minutes).
const DEFAULT_HTTP_REQ_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Release scraper.
#[derive(Clone, Debug)]
pub struct Scraper {
    stream: String,
    /// arch -> graph
    graphs: HashMap<String, Bytes>,
    /// arch -> graph
    oci_graphs: HashMap<String, Bytes>,
    hclient: reqwest::Client,
    pause_secs: NonZeroU64,
    release_index_url: reqwest::Url,
    updates_url: reqwest::Url,
}

impl Scraper {
    pub(crate) fn new(stream: String, arches: Vec<String>) -> Fallible<Self> {
        let empty = {
            let empty_graph = graph::Graph::default();
            let data = serde_json::to_vec(&empty_graph)?;
            Bytes::from(data)
        };
        let graphs = arches
            .iter()
            .map(|arch| (arch.clone(), empty.clone()))
            .collect();
        let oci_graphs = arches
            .into_iter()
            .map(|arch| (arch, empty.clone()))
            .collect();

        let vars = maplit::hashmap! {
            "stream".to_string() => stream.clone(),
        };
        let releases_json = envsubst::substitute(metadata::RELEASES_JSON, &vars)?;
        let updates_json = envsubst::substitute(metadata::UPDATES_JSON, &vars)?;
        let hclient = reqwest::ClientBuilder::new()
            .pool_idle_timeout(Some(Duration::from_secs(10)))
            .timeout(DEFAULT_HTTP_REQ_TIMEOUT)
            .build()?;

        let scraper = Self {
            graphs,
            oci_graphs,
            hclient,
            pause_secs: NonZeroU64::new(30).expect("non-zero pause"),
            stream,
            release_index_url: reqwest::Url::parse(&releases_json)?,
            updates_url: reqwest::Url::parse(&updates_json)?,
        };
        Ok(scraper)
    }

    /// Return a request builder with base URL and parameters set.
    fn new_request(
        &self,
        method: reqwest::Method,
        url: reqwest::Url,
    ) -> Fallible<reqwest::RequestBuilder> {
        log::trace!("building new request for {url}");
        let builder = self.hclient.request(method, url);
        Ok(builder)
    }

    /// Fetch releases from release-index.
    fn fetch_releases(&self) -> impl Future<Output = Result<Vec<metadata::Release>, Error>> {
        let target = self.release_index_url.clone();
        let req = self.new_request(Method::GET, target);

        async {
            let resp = req?.send().await?;
            let content = resp.error_for_status()?;
            let json = content.json::<metadata::ReleasesJSON>().await?;
            Ok(json.releases)
        }
    }

    /// Fetch updates metadata.
    fn fetch_updates(&self) -> impl Future<Output = Result<metadata::UpdatesJSON, Error>> {
        let target = self.updates_url.clone();
        let req = self.new_request(Method::GET, target);

        async {
            let resp = req?.send().await?;
            let content = resp.error_for_status()?;
            let json = content.json::<metadata::UpdatesJSON>().await?;
            Ok(json)
        }
    }

    /// Combine release-index and updates metadata.
    fn assemble_graphs(
        &self,
    ) -> impl Future<
        Output = Result<(HashMap<String, graph::Graph>, HashMap<String, graph::Graph>), Error>,
    > {
        let stream_releases = self.fetch_releases();
        let stream_updates = self.fetch_updates();

        // yuck... we clone a bunch here to keep the async closure 'static
        let stream = self.stream.clone();
        let arches: Vec<String> = self.graphs.keys().cloned().collect();

        async move {
            let (graph, updates) =
                futures::future::try_join(stream_releases, stream_updates).await?;
            // first the legacy graphs
            let mut map = HashMap::with_capacity(arches.len());
            for arch in &arches {
                map.insert(
                    arch.clone(),
                    graph::Graph::from_metadata(
                        graph.clone(),
                        updates.clone(),
                        graph::GraphScope {
                            basearch: arch.clone(),
                            stream: stream.clone(),
                            oci: false,
                        },
                    )?,
                );
            }
            // now the OCI graphs
            let mut oci_map = HashMap::with_capacity(arches.len());
            for arch in &arches {
                oci_map.insert(
                    arch.clone(),
                    graph::Graph::from_metadata(
                        graph.clone(),
                        updates.clone(),
                        graph::GraphScope {
                            basearch: arch.clone(),
                            stream: stream.clone(),
                            oci: true,
                        },
                    )?,
                );
            }
            Ok((map, oci_map))
        }
    }

    /// Update cached graph.
    fn update_cached_graph(
        &mut self,
        arch: String,
        oci: bool,
        graph: graph::Graph,
    ) -> Result<(), Error> {
        let data = serde_json::to_vec_pretty(&graph).map_err(|e| failure::format_err!("{}", e))?;
        let graph_type = if oci { "oci" } else { "checksum" };

        let refresh_timestamp = chrono::Utc::now();
        crate::LAST_REFRESH
            .with_label_values(&[&arch, &self.stream, graph_type])
            .set(refresh_timestamp.timestamp());
        crate::GRAPH_FINAL_EDGES
            .with_label_values(&[&arch, &self.stream, graph_type])
            .set(graph.edges.len() as i64);
        crate::GRAPH_FINAL_RELEASES
            .with_label_values(&[&arch, &self.stream, graph_type])
            .set(graph.nodes.len() as i64);

        log::trace!(
            "cached graph for {}/{}/oci={}: releases={}, edges={}",
            &arch,
            self.stream,
            oci,
            graph.nodes.len(),
            graph.edges.len()
        );

        if oci {
            self.oci_graphs.insert(arch, Bytes::from(data));
        } else {
            self.graphs.insert(arch, Bytes::from(data));
        }
        Ok(())
    }
}

impl Actor for Scraper {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Kick-start the state machine.
        Self::tick_now(ctx);
    }
}

pub(crate) struct RefreshTick {}

impl Message for RefreshTick {
    type Result = Result<(), failure::Error>;
}

impl Handler<RefreshTick> for Scraper {
    type Result = ResponseActFuture<Self, Result<(), failure::Error>>;

    fn handle(&mut self, _msg: RefreshTick, _ctx: &mut Self::Context) -> Self::Result {
        crate::UPSTREAM_SCRAPES
            .with_label_values(&[&self.stream])
            .inc();

        let latest_graphs = self.assemble_graphs();
        let update_graphs = actix::fut::wrap_future::<_, Self>(latest_graphs)
            .map(|graphs, actor, _ctx| {
                let res: Result<(), Error> = graphs.and_then(|(g, oci_g)| {
                    g.into_iter()
                        .map(|(arch, graph)| (arch, false, graph))
                        .chain(oci_g.into_iter().map(|(arch, graph)| (arch, true, graph)))
                        .map(|(arch, oci, graph)| actor.update_cached_graph(arch, oci, graph))
                        .collect()
                });
                if let Err(e) = res {
                    log::error!("transient scraping failure: {}", e);
                };
            })
            .then(|_r, actor, ctx| {
                let pause = Duration::from_secs(actor.pause_secs.get());
                Self::tick_later(ctx, pause);
                actix::fut::ok(())
            });

        Box::new(update_graphs)
    }
}

pub(crate) struct GetCachedGraph {
    pub(crate) scope: graph::GraphScope,
}

impl Message for GetCachedGraph {
    type Result = Result<Bytes, Error>;
}

impl Handler<GetCachedGraph> for Scraper {
    type Result = ResponseActFuture<Self, Result<Bytes, Error>>;

    fn handle(&mut self, msg: GetCachedGraph, _ctx: &mut Self::Context) -> Self::Result {
        use failure::format_err;
        let graph_type = if msg.scope.oci { "oci" } else { "checksum" };

        if msg.scope.stream != self.stream {
            return Box::new(actix::fut::err(format_err!(
                "unexpected stream '{}'",
                msg.scope.stream
            )));
        }
        let target_graphmap = if msg.scope.oci {
            &self.oci_graphs
        } else {
            &self.graphs
        };
        if let Some(graph) = target_graphmap.get(&msg.scope.basearch) {
            crate::CACHED_GRAPH_REQUESTS
                .with_label_values(&[&msg.scope.basearch, &msg.scope.stream, &graph_type])
                .inc();

            Box::new(actix::fut::ok(graph.clone()))
        } else {
            return Box::new(actix::fut::err(format_err!(
                "unexpected basearch '{}'",
                msg.scope.basearch
            )));
        }
    }
}

impl Scraper {
    /// Schedule an immediate refresh of the state machine.
    pub fn tick_now(ctx: &mut Context<Self>) {
        ctx.notify(RefreshTick {})
    }

    /// Schedule a delayed refresh of the state machine.
    pub fn tick_later(ctx: &mut Context<Self>, after: std::time::Duration) -> actix::SpawnHandle {
        ctx.notify_later(RefreshTick {}, after)
    }
}
