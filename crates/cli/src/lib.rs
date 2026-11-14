mod plan;
pub mod profile;
mod profile_file;
mod routing;
pub mod session;
pub mod sites;
pub mod system_dns;

pub use plan::{DNS_PORT, PIECE_GAP, UPSTREAM_TIMEOUT, upstream_servers};
pub use profile_file::{Problem, ProfileError, ProfileSpec, parse_domain};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
