use std::net::SocketAddr;
use std::sync::{Arc, PoisonError, RwLock};

#[derive(Debug, Clone, Default)]
pub struct Upstreams(Arc<RwLock<Vec<SocketAddr>>>);

impl Upstreams {
    pub fn new(servers: Vec<SocketAddr>) -> Self {
        Self(Arc::new(RwLock::new(servers)))
    }

    pub fn current(&self) -> Vec<SocketAddr> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn replace(&self, servers: Vec<SocketAddr>) -> bool {
        let mut held = self.0.write().unwrap_or_else(PoisonError::into_inner);
        if *held == servers {
            return false;
        }
        *held = servers;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(text: &str) -> SocketAddr {
        text.parse().unwrap()
    }

    #[test]
    fn a_clone_sees_a_replacement_made_through_the_original() {
        let upstreams = Upstreams::new(vec![server("192.0.2.1:53")]);
        let seen_by_resolver = upstreams.clone();
        assert!(upstreams.replace(vec![server("198.51.100.7:53"), server("192.0.2.1:53")]));
        assert_eq!(
            seen_by_resolver.current(),
            vec![server("198.51.100.7:53"), server("192.0.2.1:53")]
        );
    }

    #[test]
    fn replacing_with_the_same_list_reports_no_change() {
        let upstreams = Upstreams::new(vec![server("192.0.2.1:53"), server("192.0.2.2:53")]);
        assert!(!upstreams.replace(vec![server("192.0.2.1:53"), server("192.0.2.2:53")]));
        assert!(upstreams.replace(vec![server("192.0.2.2:53"), server("192.0.2.1:53")]));
    }
}
