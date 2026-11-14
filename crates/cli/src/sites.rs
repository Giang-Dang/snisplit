use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use snisplit_core::domains::DomainList;

use crate::profile::{self, BUILT_IN};
use crate::profile_file::ProfileSpec;

const EXTENSION: &str = "profile";
const APP_FOLDER: &str = "snisplit";
const SITES_FOLDER: &str = "sites";
const NEW_SITE_SOURCE: &str = "new site";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteFile {
    pub name: String,
    pub loaded: Result<ProfileSpec, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    pub sites: Vec<ProfileSpec>,
    pub notes: Vec<String>,
}

struct Owner {
    label: String,
    domains: DomainList,
}

impl Owner {
    fn profile(spec: &ProfileSpec) -> Self {
        let kind = if profile::find(&spec.name).is_some() {
            "the built-in profile"
        } else {
            "the profile"
        };
        Self::labelled(format!("{kind} {}", spec.name), spec)
    }

    fn site(spec: &ProfileSpec) -> Self {
        Self::labelled(format!("the site {}", spec.name), spec)
    }

    fn labelled(label: String, spec: &ProfileSpec) -> Self {
        Self {
            label,
            domains: DomainList::new(&spec.domains),
        }
    }

    fn covers(&self, domain: &str) -> bool {
        self.domains.matched_domain(domain.as_bytes()).is_some()
    }
}

pub fn folder() -> Option<PathBuf> {
    folder_from(|key| std::env::var_os(key))
}

#[cfg(windows)]
fn folder_from(lookup: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let app_data = non_empty(lookup("APPDATA"))?;
    Some(PathBuf::from(app_data).join(APP_FOLDER).join(SITES_FOLDER))
}

#[cfg(not(windows))]
fn folder_from(lookup: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let config_home = non_empty(lookup("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| non_empty(lookup("HOME")).map(|home| PathBuf::from(home).join(".config")))?;
    Some(config_home.join(APP_FOLDER).join(SITES_FOLDER))
}

fn non_empty(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.is_empty())
}

pub fn list(folder: &Path) -> io::Result<Vec<SiteFile>> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if is_site_file(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths.iter().map(|path| read_site(path)).collect())
}

fn is_site_file(path: &Path) -> bool {
    let profile_extension = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION));
    profile_extension && path.is_file()
}

fn read_site(path: &Path) -> SiteFile {
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file_name = format!("{name}.{EXTENSION}");
    let loaded = fs::read_to_string(path)
        .map_err(|error| format!("{file_name}: cannot read it: {error}"))
        .and_then(|text| profile::parse_file(&file_name, &text).map_err(|e| e.to_string()))
        .and_then(|spec| named_after_file(&file_name, &name, spec));
    SiteFile { name, loaded }
}

fn named_after_file(file_name: &str, stem: &str, spec: ProfileSpec) -> Result<ProfileSpec, String> {
    if spec.name == stem {
        return Ok(spec);
    }
    Err(format!(
        "{file_name}: its name line says {}; a site file must be named after its profile",
        spec.name
    ))
}

pub fn admit(taken: &[ProfileSpec], files: Vec<SiteFile>) -> Admitted {
    let mut owners: Vec<Owner> = taken.iter().map(Owner::profile).collect();
    let mut admitted = Admitted {
        sites: Vec::new(),
        notes: Vec::new(),
    };
    for file in files {
        let mut spec = match file.loaded {
            Ok(spec) => spec,
            Err(problem) => {
                admitted.notes.push(format!("skipped a site: {problem}"));
                continue;
            }
        };
        let mut left_out = Vec::new();
        spec.domains
            .retain(|domain| match covering(&owners, domain) {
                Some(owner) => {
                    left_out.push(format!(
                        "site {}: {domain} left out, {owner} already covers it",
                        spec.name
                    ));
                    false
                }
                None => true,
            });
        admitted.notes.append(&mut left_out);
        if spec.domains.is_empty() {
            admitted.notes.push(format!(
                "skipped site {}: every domain is already covered",
                spec.name
            ));
            continue;
        }
        owners.push(Owner::site(&spec));
        admitted.sites.push(spec);
    }
    admitted
}

fn covering<'a>(owners: &'a [Owner], domain: &str) -> Option<&'a str> {
    owners
        .iter()
        .find(|owner| owner.covers(domain))
        .map(|owner| owner.label.as_str())
}

pub fn add(folder: &Path, profile_text: &str) -> io::Result<ProfileSpec> {
    let spec = profile::parse_file(NEW_SITE_SOURCE, profile_text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut owners: Vec<Owner> = built_in_specs()?.iter().map(Owner::profile).collect();
    owners.extend(
        list(folder)?
            .iter()
            .filter_map(|site| site.loaded.as_ref().ok())
            .map(Owner::site),
    );
    if let Some((domain, owner)) = spec
        .domains
        .iter()
        .find_map(|domain| covering(&owners, domain).map(|owner| (domain, owner)))
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{domain} is already covered by {owner}"),
        ));
    }
    fs::create_dir_all(folder)?;
    let path = folder.join(format!("{}.{EXTENSION}", spec.name));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot write site {}: {error}", spec.name),
            )
        })?;
    file.write_all(profile_text.as_bytes())?;
    Ok(spec)
}

pub fn remove(folder: &Path, name: &str) -> io::Result<()> {
    if !list(folder)?.iter().any(|site| site.name == name) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no site named {name}"),
        ));
    }
    fs::remove_file(folder.join(format!("{name}.{EXTENSION}")))
}

pub fn built_in_specs() -> io::Result<Vec<ProfileSpec>> {
    BUILT_IN
        .iter()
        .map(|built_in| built_in.spec().map_err(io::Error::other))
        .collect()
}

#[cfg(test)]
mod tests;
