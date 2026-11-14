#![forbid(unsafe_code)]

mod config;
mod connection;
pub mod dns_server;
mod event;
mod pieces;
pub mod probe;
mod relay;
mod resolve;
pub mod upstream;
mod upstreams;

pub use config::{Config, Listener};
pub use dns_server::DnsServer;
pub use event::Event;
pub use pieces::write_pieces;
pub use relay::Relay;
pub use resolve::Resolve;
pub use upstream::UdpResolver;
pub use upstreams::Upstreams;

fn binding_failed(listen: std::net::SocketAddr, error: &std::io::Error) -> std::io::Error {
    std::io::Error::new(error.kind(), format!("binding {listen} failed: {error}"))
}
