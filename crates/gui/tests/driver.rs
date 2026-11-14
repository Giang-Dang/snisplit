use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use snisplit_gui::driver::Driver;
use snisplit_gui::state::{Effect, Event, SiteKind};

const NEWS_PROFILE: &str = "name news-example-com\ndomains news.example.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n";
const WAIT: Duration = Duration::from_secs(10);

struct TempFolder(PathBuf);

impl TempFolder {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("snisplit-gui-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn driver(folder: Option<PathBuf>) -> (Driver, Receiver<Event>, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let (driver, events) = Driver::new(
        folder,
        Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .unwrap();
    (driver, events, wakes)
}

fn site_names(event: Event) -> Vec<(String, SiteKind)> {
    match event {
        Event::SitesLoaded(sites) => sites
            .into_iter()
            .map(|site| (site.name, site.kind))
            .collect(),
        other => panic!("expected the site list, got {other:?}"),
    }
}

#[test]
fn a_saved_site_is_listed_after_the_built_ins_and_a_removed_one_is_gone() {
    let folder = TempFolder::new("save");
    let (mut driver, events, wakes) = driver(Some(folder.0.clone()));
    driver.run(Effect::SaveSite(NEWS_PROFILE.to_string()));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteSaved(Ok("news-example-com".to_string()))
    );
    assert_eq!(
        fs::read_to_string(folder.0.join("news-example-com.profile")).unwrap(),
        NEWS_PROFILE
    );
    driver.run(Effect::LoadSites);
    assert_eq!(
        site_names(events.recv_timeout(WAIT).unwrap()),
        [
            ("steam".to_string(), SiteKind::BuiltIn),
            ("medium".to_string(), SiteKind::BuiltIn),
            ("news-example-com".to_string(), SiteKind::User),
        ]
    );
    driver.run(Effect::DeleteSite("news-example-com".to_string()));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteRemoved(Ok("news-example-com".to_string()))
    );
    assert!(!folder.0.join("news-example-com.profile").exists());
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
}

#[test]
fn a_save_the_library_refuses_comes_back_as_its_message() {
    let folder = TempFolder::new("refused");
    let (mut driver, events, _) = driver(Some(folder.0.clone()));
    driver.run(Effect::SaveSite(
        "name medium-com\ndomains medium.com\n".to_string(),
    ));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteSaved(Err(
            "medium.com is already covered by the built-in profile medium".to_string()
        ))
    );
}

#[test]
fn without_a_site_folder_changes_are_refused_and_only_built_ins_are_listed() {
    let (mut driver, events, _) = driver(None);
    driver.run(Effect::SaveSite(NEWS_PROFILE.to_string()));
    let refused = "no site folder: the APPDATA environment variable is not set".to_string();
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteSaved(Err(refused.clone()))
    );
    driver.run(Effect::DeleteSite("news".to_string()));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteRemoved(Err(refused))
    );
    driver.run(Effect::LoadSites);
    assert_eq!(site_names(events.recv_timeout(WAIT).unwrap()).len(), 2);
}
