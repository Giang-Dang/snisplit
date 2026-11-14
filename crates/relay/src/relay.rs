use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio::time::sleep;

use crate::binding_failed;
use crate::config::Config;
use crate::connection::{EventSink, Route, Shared, serve};
use crate::event::Event;
use crate::resolve::{Resolve, is_usable_upstream};

const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);
const UNKNOWN_HOST: &str = "";

struct Bound {
    listener: TcpListener,
    local_addr: SocketAddr,
    route: Route,
}

pub struct Relay {
    bound: Vec<Bound>,
    shared: Arc<Shared>,
}

impl Relay {
    pub async fn bind<R: Resolve>(
        config: Config,
        resolver: R,
        on_event: Arc<dyn Fn(Event) + Send + Sync>,
    ) -> io::Result<Relay> {
        Self::bind_with(config, resolver, on_event, is_usable_upstream).await
    }

    pub(crate) async fn bind_with<R: Resolve>(
        config: Config,
        resolver: R,
        on_event: EventSink,
        is_usable: fn(Ipv4Addr) -> bool,
    ) -> io::Result<Relay> {
        let routes = routes_of(&config)?;
        let mut bound = Vec::with_capacity(routes.len());
        for (listen, route) in routes {
            let listener = TcpListener::bind(listen)
                .await
                .map_err(|error| binding_failed(listen, &error))?;
            let local_addr = listener.local_addr()?;
            bound.push(Bound {
                listener,
                local_addr,
                route,
            });
        }
        let shared = Shared::new(config, Box::new(resolver), on_event, is_usable);
        Ok(Relay {
            bound,
            shared: Arc::new(shared),
        })
    }

    pub fn local_addrs(&self) -> Vec<SocketAddr> {
        self.bound.iter().map(|bound| bound.local_addr).collect()
    }

    pub async fn run(self, shutdown: impl Future<Output = ()> + Send) {
        let mut accept_loops = JoinSet::new();
        for bound in self.bound {
            let shared = Arc::clone(&self.shared);
            accept_loops.spawn(accept_loop(bound.listener, shared, bound.route));
        }
        shutdown.await;
        accept_loops.abort_all();
        while accept_loops.join_next().await.is_some() {}
    }
}

fn routes_of(config: &Config) -> io::Result<Vec<(SocketAddr, Route)>> {
    config
        .listeners
        .iter()
        .map(|listener| {
            if listener.profile >= config.profiles.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "listener {} names profile {} but there are {} profiles",
                        listener.listen,
                        listener.profile,
                        config.profiles.len()
                    ),
                ));
            }
            let route = Route {
                protocol: listener.protocol,
                profile: listener.profile,
                upstream_port: listener.upstream_port,
            };
            Ok((listener.listen, route))
        })
        .collect()
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>, route: Route) {
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((client, _peer)) => {
                connections.spawn(serve(client, Arc::clone(&shared), route));
            }
            Err(error) => {
                shared.emit(Event::Failed {
                    host: UNKNOWN_HOST.to_owned(),
                    port: route.upstream_port,
                    error: format!("accepting a connection: {error}"),
                });
                sleep(ACCEPT_RETRY_DELAY).await;
            }
        }
        while connections.try_join_next().is_some() {}
    }
}

#[cfg(test)]
mod tests;
