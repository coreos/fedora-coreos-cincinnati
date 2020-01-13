use crate::{graph, metadata};
use actix::prelude::*;
use failure::{Error, Fallible};
use reqwest::Method;
use std::num::NonZeroU64;
use std::time::Duration;

/// Release scraper.
#[derive(Clone, Debug)]
pub struct Scraper {
    graph: graph::Graph,
    hclient: reqwest::Client,
    stream: String,
    pause_secs: NonZeroU64,
    stream_metadata_url: reqwest::Url,
    release_index_url: reqwest::Url,
}

impl Scraper {
    pub fn new<S>(stream: S) -> Fallible<Self>
    where
        S: Into<String>,
    {
        let stream = stream.into();
        let vars = maplit::hashmap! { "stream".to_string() => stream.clone() };
        let releases_json = envsubst::substitute(metadata::RELEASES_JSON, &vars)?;
        let stream_json = envsubst::substitute(metadata::STREAM_JSON, &vars)?;
        let scraper = Self {
            graph: graph::Graph::default(),
            hclient: reqwest::ClientBuilder::new().build()?,
            pause_secs: NonZeroU64::new(30).expect("non-zero pause"),
            stream,
            release_index_url: reqwest::Url::parse(&releases_json)?,
            stream_metadata_url: reqwest::Url::parse(&stream_json)?,
        };
        Ok(scraper)
    }

    /// Return a request builder with base URL and parameters set.
    fn new_request(
        &self,
        method: reqwest::Method,
        url: reqwest::Url,
    ) -> Fallible<reqwest::RequestBuilder> {
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
        let target = self.stream_metadata_url.clone();
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

        // NOTE(lucab): this inner scope is in order to get a 'static lifetime on
        //  the future for actix compatibility.
        async {
            let (graph, updates) =
                futures::future::try_join(stream_releases, stream_updates).await?;
            graph::Graph::from_metadata(graph, updates)
        }
    }

    /// Update cached graph.
    fn update_cached_graph(&mut self, graph: graph::Graph) {
        self.graph = graph;

        let refresh_timestamp = chrono::Utc::now();
        crate::LAST_REFRESH
            .with_label_values(&[&self.stream])
            .set(refresh_timestamp.timestamp());
        crate::GRAPH_FINAL_EDGES
            .with_label_values(&[&self.stream])
            .set(self.graph.edges.len() as i64);
        crate::GRAPH_FINAL_RELEASES
            .with_label_values(&[&self.stream])
            .set(self.graph.nodes.len() as i64);
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

        let latest_graph = self.assemble_graph();
        let update_graph = actix::fut::wrap_future::<_, Self>(latest_graph)
            .map(|graph, actor, _ctx| {
                match graph {
                    Ok(graph) => actor.update_cached_graph(graph),
                    Err(e) => log::error!("transient scraping failure: {}", e),
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
    pub(crate) stream: String,
}

impl Default for GetCachedGraph {
    fn default() -> Self {
        Self {
            stream: "testing".to_string(),
        }
    }
}

impl Message for GetCachedGraph {
    type Result = Result<graph::Graph, Error>;
}

impl Handler<GetCachedGraph> for Scraper {
    type Result = ResponseActFuture<Self, Result<graph::Graph, Error>>;

    fn handle(&mut self, msg: GetCachedGraph, _ctx: &mut Self::Context) -> Self::Result {
        use failure::format_err;
        if msg.stream != self.stream {
            return Box::new(actix::fut::err(format_err!(
                "unexpected stream '{}'",
                msg.stream
            )));
        }
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
