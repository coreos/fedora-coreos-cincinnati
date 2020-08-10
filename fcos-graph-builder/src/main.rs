#[macro_use]
extern crate log;
#[macro_use]
extern crate prometheus;

mod scraper;

use actix::prelude::*;
use actix_web::{web, App, HttpResponse};
use commons::{metrics, policy};
use failure::{Fallible, ResultExt};
use log::LevelFilter;
use prometheus::{IntCounterVec, IntGauge, IntGaugeVec};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use structopt::clap::{crate_name, crate_version};
use structopt::StructOpt;

/// Top-level log target for this application.
static APP_LOG_TARGET: &str = "fcos_graph_builder";

lazy_static::lazy_static! {
    static ref GRAPH_FINAL_EDGES: IntGaugeVec = register_int_gauge_vec!(
        "fcos_cincinnati_gb_scraper_graph_final_edges",
        "Number of edges in the cached graph, after processing",
        &["stream"]
    ).unwrap();
    static ref GRAPH_FINAL_RELEASES: IntGaugeVec = register_int_gauge_vec!(
        "fcos_cincinnati_gb_scraper_graph_final_releases",
        "Number of releases in the cached graph, after processing",
        &["stream"]
    ).unwrap();
    static ref LAST_REFRESH: IntGaugeVec = register_int_gauge_vec!(
       "fcos_cincinnati_gb_scraper_graph_last_refresh_timestamp",
        "UTC timestamp of last graph refresh",
        &["stream"]
    ).unwrap();
    static ref UPSTREAM_SCRAPES: IntCounterVec = register_int_counter_vec!(
       "fcos_cincinnati_gb_scraper_upstream_scrapes_total",
       "Total number of upstream scrapes",
        &["stream"]
    ).unwrap();
    // NOTE(lucab): alternatively this could come from the runtime library, see
    // https://prometheus.io/docs/instrumenting/writing_clientlibs/#process-metrics
    static ref PROCESS_START_TIME: IntGauge = register_int_gauge!(opts!(
        "process_start_time_seconds",
        "Start time of the process since unix epoch in seconds."
    )).unwrap();

}

fn main() -> Fallible<()> {
    // Parse command-line options.
    let cli_opts = CliOptions::from_args();

    // Setup logging.
    env_logger::Builder::from_default_env()
        .format_timestamp_secs()
        .format_module_path(false)
        .filter(Some(APP_LOG_TARGET), cli_opts.loglevel())
        .try_init()
        .context("failed to initialize logging")?;

    debug!("command-line options:\n{:#?}", cli_opts);

    let sys = actix::System::new("fcos_cincinnati_gb");

    // TODO(lucab): figure out all configuration params.
    let allowed_origins = vec!["https://builds.coreos.fedoraproject.org"];
    let streams_cfg = maplit::btreeset!["next", "stable", "testing"];
    let mut scrapers = HashMap::with_capacity(streams_cfg.len());
    for stream in streams_cfg {
        let addr = scraper::Scraper::new(stream)?.start();
        scrapers.insert(stream.to_string(), addr);
    }

    let service_state = AppState { scrapers };

    let start_timestamp = chrono::Utc::now();
    PROCESS_START_TIME.set(start_timestamp.timestamp());
    info!("starting server ({} {})", crate_name!(), crate_version!());

    // Graph-builder service.
    let gb_service = service_state.clone();
    actix_web::HttpServer::new(move || {
        App::new()
            .wrap(commons::web::build_cors_middleware(&allowed_origins))
            .data(gb_service.clone())
            .route("/v1/graph", web::get().to(gb_serve_graph))
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 8080))?
    .run();

    // Graph-builder status service.
    let gb_status = service_state;
    actix_web::HttpServer::new(move || {
        App::new()
            .data(gb_status.clone())
            .route("/metrics", web::get().to(metrics::serve_metrics))
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 9080))?
    .run();

    sys.run()?;
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    scrapers: HashMap<String, Addr<scraper::Scraper>>,
}

#[derive(Deserialize)]
pub struct GraphQuery {
    basearch: Option<String>,
    stream: Option<String>,
}

pub(crate) async fn gb_serve_graph(
    data: actix_web::web::Data<AppState>,
    query: actix_web::web::Query<GraphQuery>,
) -> Result<HttpResponse, failure::Error> {
    let basearch = query
        .basearch
        .as_ref()
        .map(String::from)
        .unwrap_or_default();
    let stream = query.stream.as_ref().map(String::from).unwrap_or_default();

    let addr = match data.scrapers.get(&stream) {
        None => return Ok(HttpResponse::NotFound().finish()),
        Some(addr) => addr,
    };

    let cached_graph = addr.send(scraper::GetCachedGraph { stream }).await??;

    let arch_graph = policy::pick_basearch(cached_graph, basearch)?;
    let final_graph = policy::filter_deadends(arch_graph);

    let json =
        serde_json::to_string_pretty(&final_graph).map_err(|e| failure::format_err!("{}", e))?;
    let resp = HttpResponse::Ok()
        .content_type("application/json")
        .body(json);
    Ok(resp)
}

/// CLI configuration options.
#[derive(Debug, StructOpt)]
pub(crate) struct CliOptions {
    /// Verbosity level (higher is more verbose).
    #[structopt(short = "v", parse(from_occurrences))]
    verbosity: u8,

    /// Path to configuration file.
    #[structopt(short = "c")]
    pub config_path: Option<String>,
}

impl CliOptions {
    /// Returns the log-level set via command-line flags.
    pub(crate) fn loglevel(&self) -> LevelFilter {
        match self.verbosity {
            0 => LevelFilter::Warn,
            1 => LevelFilter::Info,
            2 => LevelFilter::Debug,
            _ => LevelFilter::Trace,
        }
    }
}
