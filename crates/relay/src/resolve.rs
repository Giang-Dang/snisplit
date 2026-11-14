use std::future::Future;
use std::io;
use std::net::Ipv4Addr;
use std::pin::Pin;

pub trait Resolve: Send + Sync + 'static {
    fn resolve(&self, host: &str) -> impl Future<Output = io::Result<Vec<Ipv4Addr>>> + Send;
}

pub(crate) type BoxedResolution<'a> =
    Pin<Box<dyn Future<Output = io::Result<Vec<Ipv4Addr>>> + Send + 'a>>;

pub(crate) trait ResolveBoxed: Send + Sync + 'static {
    fn resolve_boxed<'a>(&'a self, host: &'a str) -> BoxedResolution<'a>;
}

impl<R: Resolve> ResolveBoxed for R {
    fn resolve_boxed<'a>(&'a self, host: &'a str) -> BoxedResolution<'a> {
        Box::pin(self.resolve(host))
    }
}

pub(crate) fn is_usable_upstream(address: Ipv4Addr) -> bool {
    !(address.is_loopback() || address.is_unspecified() || address.is_broadcast())
}

#[cfg(test)]
mod tests;
