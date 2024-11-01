use actix::prelude::*;
use actix_web::web::Bytes;
use commons::{graph, metadata};
use failure::{Error, Fallible};
use reqwest::Method;
use std::num::NonZeroU64;
use std::time::Duration;

/// Default timeout for HTTP requests (30 minutes).
const DEFAULT_HTTP_REQ_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Release scraper.
#[derive(Clone, Debug)]
pub struct Scraper {
    graph: Bytes,
    hclient: reqwest::Client,
    pause_secs: NonZeroU64,
    release_index_url: reqwest::Url,
    scope: graph::GraphScope,
    updates_url: reqwest::Url,
}

impl Scraper {
    pub(crate) fn new(scope: graph::GraphScope) -> Fallible<Self> {
        let graph = {
            let empty_graph = graph::Graph::default();
            let data = serde_json::to_vec(&empty_graph)?;
            Bytes::from(data)
        };

        let vars = maplit::hashmap! {
            "basearch".to_string() => scope.basearch.clone(),
            "stream".to_string() => scope.stream.clone(),
        };
        let releases_json = envsubst::substitute(metadata::RELEASES_JSON, &vars)?;
        let updates_json = envsubst::substitute(metadata::UPDATES_JSON, &vars)?;
        let hclient = reqwest::ClientBuilder::new()
            .pool_idle_timeout(Some(Duration::from_secs(10)))
            .timeout(DEFAULT_HTTP_REQ_TIMEOUT)
            .build()?;

        let scraper = Self {
            graph,
            hclient,
            pause_secs: NonZeroU64::new(30).expect("non-zero pause"),
            scope,
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
    fn assemble_graph(&self) -> impl Future<Output = Result<graph::Graph, Error>> {
        let stream_releases = self.fetch_releases();
        let stream_updates = self.fetch_updates();
        let scope = self.scope.clone();

        // NOTE(lucab): this inner scope is in order to get a 'static lifetime on
        //  the future for actix compatibility.
        async {
            let (graph, updates) =
                futures::future::try_join(stream_releases, stream_updates).await?;
            graph::Graph::from_metadata(graph, updates, scope)
        }
    }

    /// Update cached graph.
    fn update_cached_graph(&mut self, graph: graph::Graph) -> Result<(), Error> {
        let data = serde_json::to_vec_pretty(&graph).map_err(|e| failure::format_err!("{}", e))?;
        self.graph = Bytes::from(data);

        let refresh_timestamp = chrono::Utc::now();
        crate::LAST_REFRESH
            .with_label_values(&[&self.scope.basearch, &self.scope.stream])
            .set(refresh_timestamp.timestamp());
        crate::GRAPH_FINAL_EDGES
            .with_label_values(&[&self.scope.basearch, &self.scope.stream])
            .set(graph.edges.len() as i64);
        crate::GRAPH_FINAL_RELEASES
            .with_label_values(&[&self.scope.basearch, &self.scope.stream])
            .set(graph.nodes.len() as i64);

        log::trace!(
            "cached graph for {}/{}: releases={}, edges={}",
            self.scope.basearch,
            self.scope.stream,
            graph.nodes.len(),
            graph.edges.len()
        );

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
            .with_label_values(&[&self.scope.basearch, &self.scope.stream])
            .inc();

        let latest_graph = self.assemble_graph();
        let update_graph = actix::fut::wrap_future::<_, Self>(latest_graph)
            .map(|graph, actor, _ctx| {
                let res = graph.and_then(|g| actor.update_cached_graph(g));
                if let Err(e) = res {
                    log::error!("transient scraping failure: {}", e);
                };
            })
            .then(|_r, actor, ctx| {
                let pause = Duration::from_secs(actor.pause_secs.get());
                Self::tick_later(ctx, pause);
                actix::fut::ok(())
            });

        Box::new(update_graph)
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

        if msg.scope.basearch != self.scope.basearch {
            return Box::new(actix::fut::err(format_err!(
                "unexpected basearch '{}'",
                msg.scope.basearch
            )));
        }
        if msg.scope.stream != self.scope.stream {
            return Box::new(actix::fut::err(format_err!(
                "unexpected stream '{}'",
                msg.scope.stream
            )));
        }

        crate::CACHED_GRAPH_REQUESTS
            .with_label_values(&[&self.scope.basearch, &self.scope.stream])
            .inc();

        Box::new(actix::fut::ok(self.graph.clone()))
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
