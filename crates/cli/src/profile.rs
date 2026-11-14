use snisplit_core::domains::DomainList;
use snisplit_core::first_flight::{HttpMethod, RelayProfile};
use snisplit_core::strategy::SplitPosition;

pub const STEAM_DOMAINS: [&str; 14] = [
    "steampowered.com",
    "steamcommunity.com",
    "steamstatic.com",
    "steamcontent.com",
    "steamserver.net",
    "steam-chat.com",
    "steamgames.com",
    "steamusercontent.com",
    "steamcdn-a.akamaihd.net",
    "steamuserimages-a.akamaihd.net",
    "steamvideo-a.akamaihd.net",
    "steambroadcast.akamaized.net",
    "steambroadcast-test.akamaized.net",
    "valvesoftware.com",
];

pub const MEDIUM_DOMAINS: [&str; 1] = ["medium.com"];

const HTTPS_PORT: u16 = 443;
const HTTP_PORT: u16 = 80;
const STEAM_CLIENT_TLS_PORTS: std::ops::RangeInclusive<u16> = 27015..=27030;

pub struct BuiltIn {
    pub name: &'static str,
    pub domains: &'static [&'static str],
    pub profile: fn() -> RelayProfile,
}

pub const BUILT_IN: [BuiltIn; 2] = [
    BuiltIn {
        name: "steam",
        domains: &STEAM_DOMAINS,
        profile: steam,
    },
    BuiltIn {
        name: "medium",
        domains: &MEDIUM_DOMAINS,
        profile: medium,
    },
];

pub fn steam() -> RelayProfile {
    RelayProfile {
        name: "steam".to_string(),
        domains: DomainList::new(STEAM_DOMAINS),
        tls_positions: vec![SplitPosition::Offset(1), SplitPosition::DomainMid],
        tls_ports: std::iter::once(HTTPS_PORT)
            .chain(STEAM_CLIENT_TLS_PORTS)
            .collect(),
        http_ports: vec![HTTP_PORT],
        http_method: HttpMethod::HostSpell,
    }
}

pub fn medium() -> RelayProfile {
    RelayProfile {
        name: "medium".to_string(),
        domains: DomainList::new(MEDIUM_DOMAINS),
        tls_positions: vec![SplitPosition::Offset(1)],
        tls_ports: vec![HTTPS_PORT],
        http_ports: vec![HTTP_PORT],
        http_method: HttpMethod::HostSpell,
    }
}

pub fn find(name: &str) -> Option<&'static BuiltIn> {
    BUILT_IN.iter().find(|built_in| built_in.name == name)
}

#[cfg(test)]
mod tests;
