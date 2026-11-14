use core::ops::Range;

pub struct DomainList {
    domains: Vec<String>,
}

impl DomainList {
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(domains: I) -> Self {
        Self {
            domains: domains.into_iter().map(|d| d.as_ref().to_owned()).collect(),
        }
    }

    pub fn matched_domain(&self, _host: &[u8]) -> Option<Range<usize>> {
        let _ = &self.domains;
        todo!()
    }
}
