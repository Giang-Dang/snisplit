use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::*;

const NEWS_SITE: &str = "name news-example-com\ndomains news.example.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n";

struct TempFolder {
    app: PathBuf,
    sites: PathBuf,
}

impl TempFolder {
    fn new(tag: &str) -> Self {
        let app = std::env::temp_dir().join(format!("snisplit-sites-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&app);
        let sites = app.join("sites");
        Self { app, sites }
    }

    fn created(tag: &str) -> Self {
        let folder = Self::new(tag);
        fs::create_dir_all(&folder.sites).unwrap();
        folder
    }

    fn write(&self, file_name: &str, text: &str) {
        fs::write(self.sites.join(file_name), text).unwrap();
    }

    fn path(&self) -> &Path {
        &self.sites
    }

    fn off_list(&self) -> PathBuf {
        self.app.join("off.list")
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.app);
    }
}

fn site(name: &str, domains: &[&str]) -> SiteFile {
    let text = format!("name {name}\ndomains {}\n", domains.join(" "));
    SiteFile {
        name: name.to_string(),
        loaded: profile::parse_file(name, &text).map_err(|error| error.to_string()),
    }
}

fn domains_of(spec: &ProfileSpec) -> Vec<&str> {
    spec.domains.iter().map(String::as_str).collect()
}

#[cfg(windows)]
#[test]
fn the_folder_is_snisplit_sites_under_appdata_and_absent_without_it() {
    let appdata =
        |value: &'static str| move |key: &str| (key == "APPDATA").then(|| OsString::from(value));
    assert_eq!(
        folder_from(appdata(r"D:\Profiles\Roaming")),
        Some(PathBuf::from(r"D:\Profiles\Roaming\snisplit\sites"))
    );
    assert_eq!(folder_from(appdata("")), None);
    assert_eq!(
        folder_from(|key: &str| (key == "HOME").then(|| OsString::from(r"D:\home"))),
        None
    );
}

#[cfg(not(windows))]
#[test]
fn the_folder_follows_xdg_config_home_when_absolute_and_home_otherwise() {
    let both = |config: &'static str| {
        move |key: &str| match key {
            "XDG_CONFIG_HOME" => Some(OsString::from(config)),
            "HOME" => Some(OsString::from("/home/owner")),
            _ => None,
        }
    };
    assert_eq!(
        folder_from(both("/etc/xdg-owner")),
        Some(PathBuf::from("/etc/xdg-owner/snisplit/sites"))
    );
    assert_eq!(
        folder_from(both("relative/config")),
        Some(PathBuf::from("/home/owner/.config/snisplit/sites"))
    );
    assert_eq!(folder_from(|_: &str| None), None);
}

#[test]
fn a_missing_folder_lists_no_sites_and_is_not_an_error() {
    let folder = TempFolder::new("missing");
    assert_eq!(list(folder.path()).unwrap(), Vec::new());
}

#[test]
fn the_list_holds_every_profile_file_by_file_name_with_problems_named_by_file() {
    let folder = TempFolder::created("list");
    folder.write("zeta.profile", "name zeta\ndomains zeta.example.net\n");
    folder.write(
        "alpha.profile",
        "name alpha\ndomains alpha.example.org a2.example.org\n",
    );
    folder.write("notes.txt", "name notes\ndomains notes.example.org\n");
    folder.write("broken.profile", "name broken\ncolour blue\n");
    folder.write("renamed.profile", "name other\ndomains other.example.org\n");
    let listed = list(folder.path()).unwrap();
    let names: Vec<&str> = listed.iter().map(|site| site.name.as_str()).collect();
    assert_eq!(names, ["alpha", "broken", "renamed", "zeta"]);
    let alpha = listed[0].loaded.as_ref().unwrap();
    assert_eq!(domains_of(alpha), ["alpha.example.org", "a2.example.org"]);
    assert_eq!(
        listed[1].loaded,
        Err("broken.profile:2: unknown key \"colour\"".to_string())
    );
    assert_eq!(
        listed[2].loaded,
        Err(
            "renamed.profile: its name line says other; a site file must be named after its profile"
                .to_string()
        )
    );
    assert_eq!(
        domains_of(listed[3].loaded.as_ref().unwrap()),
        ["zeta.example.net"]
    );
}

#[test]
fn a_site_named_like_a_built_in_is_a_problem_of_that_file() {
    let folder = TempFolder::created("reserved");
    folder.write(
        "medium.profile",
        "name medium\ndomains medium.example.org\n",
    );
    let listed = list(folder.path()).unwrap();
    assert_eq!(
        listed[0].loaded,
        Err(
            "medium.profile:1: bad name \"medium\": it is the name of a built-in profile"
                .to_string()
        )
    );
}

#[test]
fn admitting_drops_domains_a_built_in_or_an_earlier_site_covers_and_skips_emptied_sites() {
    let taken = built_in_specs().unwrap();
    let broken = SiteFile {
        name: "broken".to_string(),
        loaded: Err("broken.profile:2: unknown key \"colour\"".to_string()),
    };
    let files = vec![
        broken,
        site(
            "news",
            &["news.example.com", "cdn.medium.com", "notmedium.com"],
        ),
        site("store", &["store.steampowered.com", "steampowered.com"]),
        site("later", &["news.example.com", "later.example.com"]),
    ];
    let admitted = admit(&taken, files);
    let kept: Vec<(&str, Vec<&str>)> = admitted
        .sites
        .iter()
        .map(|spec| (spec.name.as_str(), domains_of(spec)))
        .collect();
    assert_eq!(
        kept,
        [
            ("news", vec!["news.example.com", "notmedium.com"]),
            ("later", vec!["later.example.com"]),
        ]
    );
    assert_eq!(
        admitted.notes,
        [
            "skipped a site: broken.profile:2: unknown key \"colour\"",
            "site news: cdn.medium.com left out, the built-in profile medium already covers it",
            "site store: store.steampowered.com left out, the built-in profile steam already covers it",
            "site store: steampowered.com left out, the built-in profile steam already covers it",
            "skipped site store: every domain is already covered",
            "site later: news.example.com left out, the site news already covers it",
        ]
    );
}

#[test]
fn an_added_site_is_saved_under_its_name_and_listed_with_the_probe_text() {
    let folder = TempFolder::new("add");
    let added = add(folder.path(), NEWS_SITE).unwrap();
    assert_eq!(added.name, "news-example-com");
    assert_eq!(
        fs::read_to_string(folder.path().join("news-example-com.profile")).unwrap(),
        NEWS_SITE
    );
    let listed = list(folder.path()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "news-example-com");
    assert_eq!(
        domains_of(listed[0].loaded.as_ref().unwrap()),
        ["news.example.com"]
    );
}

#[test]
fn adding_a_domain_already_covered_is_refused_and_writes_nothing() {
    let folder = TempFolder::new("covered");
    let steam = "name store-steampowered-com\ndomains store.steampowered.com\n";
    let refused = add(folder.path(), steam).unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        refused.to_string(),
        "store.steampowered.com is already covered by the built-in profile steam"
    );
    let by_steam = covered(&refused).unwrap();
    assert_eq!(
        (by_steam.domain.as_str(), by_steam.owner.as_str()),
        ("store.steampowered.com", "steam")
    );
    assert!(!folder.path().exists());
    add(folder.path(), NEWS_SITE).unwrap();
    let again = add(folder.path(), "name news2\ndomains a.news.example.com\n").unwrap_err();
    assert_eq!(
        again.to_string(),
        "a.news.example.com is already covered by the site news-example-com"
    );
    assert_eq!(covered(&again).unwrap().owner, "news-example-com");
    assert_eq!(list(folder.path()).unwrap().len(), 1);
    assert!(covered(&io::Error::other("not a coverage refusal")).is_none());
}

#[test]
fn the_off_list_sits_in_the_snisplit_folder_and_a_missing_one_switches_nothing_off() {
    let folder = TempFolder::new("off-missing");
    assert_eq!(off_list_path(folder.path()), folder.off_list());
    assert_eq!(switched_off(folder.path()).unwrap(), Vec::<String>::new());
}

#[test]
fn a_switch_writes_one_known_name_per_line_and_drops_names_that_match_no_site() {
    let folder = TempFolder::created("off-write");
    folder.write("news.profile", "name news\ndomains news.example.com\n");
    fs::write(folder.off_list(), b"ghost\n  medium \nmedium\n\xff\xfe\n").unwrap();
    assert_eq!(
        switched_off(folder.path()).unwrap()[..2],
        ["ghost", "medium"]
    );
    set_switch(folder.path(), "news", false).unwrap();
    assert_eq!(
        fs::read_to_string(folder.off_list()).unwrap(),
        "medium\nnews\n"
    );
    set_switch(folder.path(), "medium", true).unwrap();
    assert_eq!(fs::read_to_string(folder.off_list()).unwrap(), "news\n");
    let unknown = set_switch(folder.path(), "nobody", false).unwrap_err();
    assert_eq!(unknown.kind(), io::ErrorKind::NotFound);
    assert_eq!(unknown.to_string(), "no site named nobody");
    assert_eq!(fs::read_to_string(folder.off_list()).unwrap(), "news\n");
    assert!(!folder.app.join("off.list.new").exists());
}

#[test]
fn a_built_in_can_be_switched_off_before_any_site_folder_exists() {
    let folder = TempFolder::new("off-built-in");
    set_switch(folder.path(), "steam", false).unwrap();
    assert_eq!(fs::read_to_string(folder.off_list()).unwrap(), "steam\n");
    assert_eq!(switched_off(folder.path()).unwrap(), ["steam"]);
}

#[test]
fn a_site_removed_while_switched_off_comes_back_on_when_added_again() {
    let folder = TempFolder::new("off-readd");
    add(folder.path(), NEWS_SITE).unwrap();
    set_switch(folder.path(), "news-example-com", false).unwrap();
    remove(folder.path(), "news-example-com").unwrap();
    assert_eq!(switched_off(folder.path()).unwrap(), ["news-example-com"]);
    add(folder.path(), NEWS_SITE).unwrap();
    assert_eq!(switched_off(folder.path()).unwrap(), Vec::<String>::new());
    assert_eq!(fs::read_to_string(folder.off_list()).unwrap(), "");
}

#[test]
fn adding_text_that_does_not_parse_or_reuses_a_file_name_is_refused() {
    let folder = TempFolder::created("bad-add");
    let unparsed = add(folder.path(), "name x\ncolour blue\n").unwrap_err();
    assert_eq!(unparsed.kind(), io::ErrorKind::InvalidData);
    assert_eq!(unparsed.to_string(), "new site:2: unknown key \"colour\"");
    folder.write("taken.profile", "this file is broken\n");
    let clash = add(folder.path(), "name taken\ndomains taken.example.org\n").unwrap_err();
    assert_eq!(clash.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read_to_string(folder.path().join("taken.profile")).unwrap(),
        "this file is broken\n"
    );
}

#[test]
fn a_failed_write_removes_the_partial_file_and_returns_the_write_error() {
    let folder = TempFolder::created("partial");
    let path = folder.path().join("half.profile");
    let failure = write_or_remove(File::create_new(&path).unwrap(), &path, |file| {
        file.write_all(b"name ha")?;
        assert_eq!(fs::read_to_string(&path).unwrap(), "name ha");
        Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"))
    })
    .unwrap_err();
    assert_eq!(failure.kind(), io::ErrorKind::StorageFull);
    assert_eq!(failure.to_string(), "disk full");
    assert!(!path.exists());
    let retry = File::create_new(&path).unwrap();
    write_or_remove(retry, &path, |file| file.write_all(b"name half\n")).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "name half\n");
}

#[test]
fn two_domains_with_the_same_site_name_give_a_name_exists_error_and_keep_the_first_file() {
    let folder = TempFolder::new("same-name");
    let first = "name foo-bar-example-com\ndomains foo-bar.example.com\n";
    let second = "name foo-bar-example-com\ndomains foo.bar-example.com\n";
    add(folder.path(), first).unwrap();
    let refused = add(folder.path(), second).unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        refused.to_string(),
        "a site named foo-bar-example-com already exists"
    );
    assert_eq!(
        fs::read_to_string(folder.path().join("foo-bar-example-com.profile")).unwrap(),
        first
    );
}

#[test]
fn removing_deletes_only_a_listed_site_file() {
    let folder = TempFolder::created("remove");
    let outside = TempFolder::created("remove-outside");
    outside.write("x.profile", "name x\ndomains x.example.org\n");
    folder.write("keep.profile", "name keep\ndomains keep.example.org\n");
    folder.write("gone.profile", "name gone\ndomains gone.example.org\n");
    remove(folder.path(), "gone").unwrap();
    let names: Vec<String> = list(folder.path())
        .unwrap()
        .into_iter()
        .map(|site| site.name)
        .collect();
    assert_eq!(names, ["keep"]);
    let escape = format!(
        "../../{}/sites/x",
        outside.app.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(
        remove(folder.path(), &escape).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert!(outside.path().join("x.profile").exists());
    assert_eq!(
        remove(folder.path(), "gone").unwrap_err().to_string(),
        "no site named gone"
    );
}
