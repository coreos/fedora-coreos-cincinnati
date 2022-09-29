#[macro_use]
extern crate log;
#[macro_use]
extern crate prometheus;

mod cli;
mod config;
mod scraper;
mod settings;

use actix::prelude::*;
use actix_web::{web, App, HttpResponse};
use clap::{crate_name, crate_version, Parser};
use commons::{graph, metrics};
use failure::{Fallible, ResultExt};
use prometheus::{IntCounterVec, IntGauge, IntGaugeVec};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

/// Top-level log target for this application.
static APP_LOG_TARGET: &str = "fcos_graph_builder";

lazy_static::lazy_static! {
    static ref CACHED_GRAPH_REQUESTS: IntCounterVec = register_int_counter_vec!(
        "fcos_cincinnati_gb_cache_graph_requests_total",
        "Total number of requests for a cached graph",
        &["basearch", "stream"]
    ).unwrap();
    static ref GRAPH_FINAL_EDGES: IntGaugeVec = register_int_gauge_vec!(
        "fcos_cincinnati_gb_scraper_graph_final_edges",
        "Number of edges in the cached graph, after processing",
        &["basearch", "stream"]
    ).unwrap();
    static ref GRAPH_FINAL_RELEASES: IntGaugeVec = register_int_gauge_vec!(
        "fcos_cincinnati_gb_scraper_graph_final_releases",
        "Number of releases in the cached graph, after processing",
        &["basearch", "stream"]
    ).unwrap();
    static ref LAST_REFRESH: IntGaugeVec = register_int_gauge_vec!(
       "fcos_cincinnati_gb_scraper_graph_last_refresh_timestamp",
        "UTC timestamp of last graph refresh",
        &["basearch", "stream"]
    ).unwrap();
    static ref UPSTREAM_SCRAPES: IntCounterVec = register_int_counter_vec!(
       "fcos_cincinnati_gb_scraper_upstream_scrapes_total",
       "Total number of upstream scrapes",
        &["basearch", "stream"]
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
    let cli_opts = cli::CliOptions::parse();

    // Setup logging.
    env_logger::Builder::from_default_env()
        .format_timestamp_secs()
        .format_module_path(false)
        .filter(Some(APP_LOG_TARGET), cli_opts.loglevel())
        .try_init()
        .context("failed to initialize logging")?;

    let sys = actix::System::new("fcos_cincinnati_gb");

    // Parse config file and validate settings.
    let (service_settings, status_settings) = {
        debug!("config file location: {}", cli_opts.config_path.display());
        let cfg = config::FileConfig::parse_file(cli_opts.config_path)?;
        let settings = settings::GraphBuilderSettings::validate_config(cfg)?;
        (settings.service, settings.status)
    };

    let mut scrapers = HashMap::with_capacity(service_settings.scopes.len());
    for scope in &service_settings.scopes {
        let addr = scraper::Scraper::new(scope.clone())?.start();
        scrapers.insert(scope.clone(), addr);
    }

    // TODO(lucab): get allowed scopes from config file.
    let service_state = AppState {
        scope_filter: None,
        scrapers,
    };

    let start_timestamp = chrono::Utc::now();
    PROCESS_START_TIME.set(start_timestamp.timestamp());
    info!("starting server ({} {})", crate_name!(), crate_version!());

    // Graph-builder main service.
    let service_socket = service_settings.socket_addr();
    debug!("main service address: {}", service_socket);
    let gb_service = service_state.clone();
    actix_web::HttpServer::new(move || {
        App::new()
            .wrap(commons::web::build_cors_middleware(
                &service_settings.origin_allowlist,
            ))
            .data(gb_service.clone())
            .route("/v1/graph", web::get().to(gb_serve_graph))
    })
    .bind(service_socket)?
    .run();

    // Graph-builder status service.
    let status_socket = status_settings.socket_addr();
    debug!("status service address: {}", status_socket);
    let gb_status = service_state;
    actix_web::HttpServer::new(move || {
        App::new()
            .data(gb_status.clone())
            .route("/metrics", web::get().to(metrics::serve_metrics))
    })
    .bind(status_socket)?
    .run();

    sys.run()?;
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    scope_filter: Option<HashSet<graph::GraphScope>>,
    scrapers: HashMap<graph::GraphScope, Addr<scraper::Scraper>>,
}

/// Mandatory parameters for querying a graph from graph-builder.
#[derive(Deserialize)]
struct GraphQuery {
    basearch: Option<String>,
    stream: Option<String>,
}

pub(crate) async fn gb_serve_graph(
    data: web::Data<AppState>,
    web::Query(query): web::Query<GraphQuery>,
) -> Result<HttpResponse, failure::Error> {
    let scope = match commons::web::validate_scope(query.basearch, query.stream, &data.scope_filter)
    {
        Err(e) => {
            log::error!("graph request with invalid scope: {}", e);
            return Ok(HttpResponse::BadRequest().finish());
        }
        Ok(s) => {
            log::trace!(
                "serving request for valid scope: basearch='{}', stream='{}'",
                s.basearch,
                s.stream
            );
            s
        }
    };

    let addr = match data.scrapers.get(&scope) {
        None => {
            log::error!(
                "no scraper configured for scope: basearch='{}', stream='{}'",
                scope.basearch,
                scope.stream,
            );
            return Ok(HttpResponse::NotFound().finish());
        }
        Some(addr) => addr,
    };

    let graph_json_bytes = addr.send(scraper::GetCachedGraph { scope }).await??;

    let resp = HttpResponse::Ok()
        .content_type("application/json")
        .body(graph_json_bytes);
    Ok(resp)
}
