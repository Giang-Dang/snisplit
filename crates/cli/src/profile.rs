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

pub fn find(name: &str) -> Option<&'static BuiltIn> {
    BUILT_IN.iter().find(|built_in| built_in.name == name)
}

#[cfg(test)]
mod tests;
