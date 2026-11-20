use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use snisplit_core::domains::DomainList;

use crate::profile::{self, BUILT_IN};
use crate::profile_file::ProfileSpec;

const EXTENSION: &str = "profile";
const APP_FOLDER: &str = "snisplit";
const SITES_FOLDER: &str = "sites";
const NEW_SITE_SOURCE: &str = "new site";
const OFF_LIST: &str = "off.list";
const OFF_LIST_DRAFT: &str = "off.list.new";
const BYTE_ORDER_MARK: char = '\u{feff}';
#[cfg(not(windows))]
const CONFIG_FOLDER: &str = ".config";
#[cfg(target_os = "linux")]
const PASSWD_FILE: &str = "/etc/passwd";
#[cfg(target_os = "linux")]
const PASSWD_SEPARATOR: char = ':';
#[cfg(target_os = "linux")]
const PASSWD_UID_FIELD: usize = 2;
#[cfg(target_os = "linux")]
const PASSWD_HOME_FIELD: usize = 5;
#[cfg(target_os = "linux")]
const INVOKER_UID_VARIABLES: [&str; 2] = ["SUDO_UID", "PKEXEC_UID"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteFile {
    pub name: String,
    pub loaded: Result<ProfileSpec, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Admitted {
    pub sites: Vec<ProfileSpec>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Covered {
    pub domain: String,
    pub owner: String,
    label: String,
}

impl fmt::Display for Covered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is already covered by {}", self.domain, self.label)
    }
}

impl std::error::Error for Covered {}

pub fn covered(error: &io::Error) -> Option<&Covered> {
    error.get_ref()?.downcast_ref::<Covered>()
}

struct Owner {
    name: String,
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
            name: spec.name.clone(),
            label,
            domains: DomainList::new(&spec.domains),
        }
    }

    fn covers(&self, domain: &str) -> bool {
        self.domains.matched_domain(domain.as_bytes()).is_some()
    }
}

#[cfg(not(target_os = "linux"))]
pub fn folder() -> Option<PathBuf> {
    folder_from(|key| std::env::var_os(key))
}

#[cfg(target_os = "linux")]
pub fn folder() -> Option<PathBuf> {
    folder_of_invoker(
        |key| std::env::var_os(key),
        || fs::read_to_string(PASSWD_FILE).ok(),
    )
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
        .or_else(|| {
            non_empty(lookup("HOME")).map(|home| PathBuf::from(home).join(CONFIG_FOLDER))
        })?;
    Some(sites_in(config_home))
}

#[cfg(not(windows))]
fn sites_in(config_home: PathBuf) -> PathBuf {
    config_home.join(APP_FOLDER).join(SITES_FOLDER)
}

#[cfg(target_os = "linux")]
fn folder_of_invoker(
    lookup: impl Fn(&str) -> Option<OsString>,
    read_passwd: impl Fn() -> Option<String>,
) -> Option<PathBuf> {
    invoking_uid(&lookup)
        .and_then(|uid| home_of_uid(&read_passwd()?, uid))
        .map(|home| sites_in(home.join(CONFIG_FOLDER)))
        .or_else(|| folder_from(lookup))
}

#[cfg(target_os = "linux")]
fn invoking_uid(lookup: &impl Fn(&str) -> Option<OsString>) -> Option<u32> {
    INVOKER_UID_VARIABLES
        .iter()
        .find_map(|key| non_empty(lookup(key)))?
        .to_str()?
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
fn home_of_uid(passwd: &str, uid: u32) -> Option<PathBuf> {
    passwd.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(PASSWD_SEPARATOR).collect();
        let entry_uid: u32 = fields.get(PASSWD_UID_FIELD)?.parse().ok()?;
        let home = PathBuf::from(fields.get(PASSWD_HOME_FIELD)?);
        (entry_uid == uid && home.is_absolute()).then_some(home)
    })
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

pub(crate) fn admit(taken: &[ProfileSpec], files: Vec<SiteFile>) -> Admitted {
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
    if let Some((domain, owner)) = spec.domains.iter().find_map(|domain| {
        owners
            .iter()
            .find(|owner| owner.covers(domain))
            .map(|owner| (domain, owner))
    }) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            Covered {
                domain: domain.clone(),
                owner: owner.name.clone(),
                label: owner.label.clone(),
            },
        ));
    }
    let off = switched_off(folder)?;
    fs::create_dir_all(folder)?;
    let path = folder.join(format!("{}.{EXTENSION}", spec.name));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| match error.kind() {
            io::ErrorKind::AlreadyExists => io::Error::new(
                error.kind(),
                format!("a site named {} already exists", spec.name),
            ),
            kind => io::Error::new(kind, format!("cannot write site {}: {error}", spec.name)),
        })?;
    write_or_remove(file, &path, |file| {
        file.write_all(profile_text.as_bytes())?;
        file.flush()
    })?;
    if off.contains(&spec.name) {
        let kept: Vec<String> = off.into_iter().filter(|name| *name != spec.name).collect();
        write_off_list(folder, &kept).inspect_err(|_| {
            let _ = fs::remove_file(&path);
        })?;
    }
    Ok(spec)
}

fn write_or_remove(
    mut file: File,
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let written = write(&mut file);
    drop(file);
    if written.is_err() {
        let _ = fs::remove_file(path);
    }
    written
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

pub fn off_list_path(folder: &Path) -> PathBuf {
    folder.parent().unwrap_or(folder).join(OFF_LIST)
}

pub fn switched_off(folder: &Path) -> io::Result<Vec<String>> {
    let bytes = match fs::read(off_list_path(folder)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut names: Vec<String> = Vec::new();
    for line in text.strip_prefix(BYTE_ORDER_MARK).unwrap_or(&text).lines() {
        let name = line.trim();
        if !name.is_empty() && !names.iter().any(|known| known == name) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

pub fn set_switch(folder: &Path, name: &str, on: bool) -> io::Result<()> {
    let mut known: Vec<String> = BUILT_IN
        .iter()
        .map(|built_in| built_in.name.to_string())
        .collect();
    known.extend(list(folder)?.into_iter().map(|site| site.name));
    if !known.iter().any(|known| known == name) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no site named {name}"),
        ));
    }
    let off = switched_off(folder)?;
    let kept: Vec<String> = known
        .into_iter()
        .filter(|known| {
            if known == name {
                !on
            } else {
                off.contains(known)
            }
        })
        .collect();
    write_off_list(folder, &kept)
}

fn write_off_list(folder: &Path, names: &[String]) -> io::Result<()> {
    let path = off_list_path(folder);
    let app_folder = folder.parent().unwrap_or(folder);
    fs::create_dir_all(app_folder)?;
    let draft = app_folder.join(OFF_LIST_DRAFT);
    let text: String = names.iter().map(|name| format!("{name}\n")).collect();
    let file = File::create(&draft)?;
    write_or_remove(file, &draft, |file| {
        file.write_all(text.as_bytes())?;
        file.sync_all()
    })?;
    fs::rename(&draft, &path).inspect_err(|_| {
        let _ = fs::remove_file(&draft);
    })
}

pub fn built_in_specs() -> io::Result<Vec<ProfileSpec>> {
    BUILT_IN
        .iter()
        .map(|built_in| built_in.spec().map_err(io::Error::other))
        .collect()
}

#[cfg(test)]
mod tests;
