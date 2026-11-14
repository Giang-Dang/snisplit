use std::net::SocketAddr;
use std::time::Duration;

use snisplit_core::first_flight::{Protocol, RelayProfile};

#[derive(Debug, Clone)]
pub struct Listener {
    pub listen: SocketAddr,
    pub upstream_port: u16,
    pub protocol: Protocol,
    pub profile: usize,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub listeners: Vec<Listener>,
    pub profiles: Vec<RelayProfile>,
    pub piece_gap: Duration,
    pub first_flight_timeout: Duration,
    pub max_first_flight: usize,
    pub connect_timeout: Duration,
}
