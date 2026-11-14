use std::fs;
use std::io;
use std::path::Path;

use crate::profile_file::{self, ProfileError, ProfileSpec};

pub struct BuiltIn {
    pub name: &'static str,
    pub text: &'static str,
}

pub const BUILT_IN: [BuiltIn; 2] = [
    BuiltIn {
        name: "steam",
        text: include_str!("../profiles/steam.profile"),
    },
    BuiltIn {
        name: "medium",
        text: include_str!("../profiles/medium.profile"),
    },
];

impl BuiltIn {
    pub fn spec(&self) -> Result<ProfileSpec, ProfileError> {
        profile_file::parse(&format!("built-in {}", self.name), self.text, &[])
    }
}

pub fn parse_file(source: &str, text: &str) -> Result<ProfileSpec, ProfileError> {
    let built_in_names: Vec<&str> = BUILT_IN.iter().map(|built_in| built_in.name).collect();
    profile_file::parse(source, text, &built_in_names)
}

pub fn read_file(path: &Path) -> io::Result<ProfileSpec> {
    let text = fs::read_to_string(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot read profile file {}: {error}", path.display()),
        )
    })?;
    parse_file(&path.display().to_string(), &text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn find(name: &str) -> Option<&'static BuiltIn> {
    BUILT_IN.iter().find(|built_in| built_in.name == name)
}

#[cfg(test)]
mod tests;
