use crate::config::FileConfig;
use commons::graph::GraphScope;
use failure::Fallible;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Runtime settings for the graph-builder.
#[derive(Clone, Debug, Default)]
pub struct GraphBuilderSettings {
    pub(crate) service: ServiceSettings,
    pub(crate) status: StatusSettings,
}

impl GraphBuilderSettings {
    pub fn validate_config(_cfg: FileConfig) -> Fallible<Self> {
        // TODO(lucab): translate config entries.
        let settings = GraphBuilderSettings::default();
        Ok(settings)
    }
}

/// Runtime settings for the main service (graph endpoint) server.
#[derive(Clone, Debug)]
pub struct ServiceSettings {
    pub(crate) origin_allowlist: Option<Vec<String>>,
    pub(crate) ip_addr: IpAddr,
    pub(crate) port: u16,
    pub(crate) scopes: BTreeSet<GraphScope>,
}

impl ServiceSettings {
    /// Default IP address for graph-builder main service.
    const DEFAULT_GB_SERVICE_ADDR: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
    /// Default TCP port for graph-builder main service.
    const DEFAULT_GB_SERVICE_PORT: u16 = 8080;
    /// Default scopes (basearch plus stream) to process.
    const DEFAULT_SCOPES: [(&'static str, &'static str); 8] = [
        ("aarch64", "next"),
        ("aarch64", "stable"),
        ("aarch64", "testing"),
        ("s390x", "next"),
        ("s390x", "testing"),
        ("x86_64", "next"),
        ("x86_64", "stable"),
        ("x86_64", "testing"),
    ];

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_addr, self.port)
    }
}

impl Default for ServiceSettings {
    fn default() -> Self {
        Self {
            origin_allowlist: None,
            ip_addr: Self::DEFAULT_GB_SERVICE_ADDR.into(),
            port: Self::DEFAULT_GB_SERVICE_PORT,
            scopes: Self::DEFAULT_SCOPES
                .iter()
                .map(|(basearch, stream)| GraphScope {
                    basearch: basearch.to_string(),
                    stream: stream.to_string(),
                })
                .collect(),
        }
    }
}

/// Runtime settings for the status server.
#[derive(Clone, Debug)]
pub struct StatusSettings {
    pub(crate) ip_addr: IpAddr,
    pub(crate) port: u16,
}

impl StatusSettings {
    /// Default IP address for graph-builder main service.
    const DEFAULT_GB_SERVICE_ADDR: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
    /// Default TCP port for graph-builder status.
    const DEFAULT_GB_STATUS_PORT: u16 = 9080;

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_addr, self.port)
    }
}

impl Default for StatusSettings {
    fn default() -> Self {
        Self {
            ip_addr: Self::DEFAULT_GB_SERVICE_ADDR.into(),
            port: Self::DEFAULT_GB_STATUS_PORT,
        }
    }
}
