use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use snisplit_gui::driver::{Driver, Folders, Keeper};
use snisplit_gui::state::{Claimed, Effect, Event, SaveRefusal, SiteKind};

const NEWS_PROFILE: &str = "name news-example-com\ndomains news.example.com\ntls-ports 443\ntls-split 1\nhttp-ports 80\nhttp-method host-spell\n";
const WAIT: Duration = Duration::from_secs(10);

struct TempFolder {
    root: PathBuf,
}

impl TempFolder {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("snisplit-gui-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        Self { root }
    }

    fn app(&self) -> PathBuf {
        self.root.join("AppData").join("snisplit")
    }

    fn sites(&self) -> PathBuf {
        self.app().join("sites")
    }

    fn folders(&self) -> Folders {
        Folders {
            sites: Some(self.sites()),
            home: Some(self.root.join("home")),
        }
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn driver(keeper: &Keeper, folders: Folders) -> (Driver<'_>, Receiver<Event>, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    let (driver, events) = keeper.driver(
        folders,
        Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }),
    );
    (driver, events, wakes)
}

fn listed(event: Event) -> Vec<(String, SiteKind, bool)> {
    match event {
        Event::SitesLoaded(sites) => sites
            .into_iter()
            .map(|site| (site.name, site.kind, site.on))
            .collect(),
        other => panic!("expected the site list, got {other:?}"),
    }
}

#[test]
fn a_saved_site_is_listed_after_the_built_ins_and_a_removed_one_is_gone() {
    let folder = TempFolder::new("save");
    let keeper = Keeper::new().unwrap();
    let (mut driver, events, wakes) = driver(&keeper, folder.folders());
    driver.run(Effect::SaveSite(NEWS_PROFILE.to_string()));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteSaved(Ok("news-example-com".to_string()))
    );
    assert_eq!(
        fs::read_to_string(folder.sites().join("news-example-com.profile")).unwrap(),
        NEWS_PROFILE
    );
    driver.run(Effect::LoadSites);
    assert_eq!(
        listed(events.recv_timeout(WAIT).unwrap()),
        [
            ("steam".to_string(), SiteKind::BuiltIn, true),
            ("medium".to_string(), SiteKind::BuiltIn, true),
            ("news-example-com".to_string(), SiteKind::User, true),
        ]
    );
    driver.run(Effect::DeleteSite("news-example-com".to_string()));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteRemoved(Ok("news-example-com".to_string()))
    );
    assert!(!folder.sites().join("news-example-com.profile").exists());
    assert_eq!(wakes.load(Ordering::SeqCst), 3);
}

#[test]
fn a_save_refused_because_the_domain_is_covered_names_the_domain_and_its_owner() {
    let folder = TempFolder::new("refused");
    let keeper = Keeper::new().unwrap();
    let (mut driver, events, _) = driver(&keeper, folder.folders());
    driver.run(Effect::SaveSite(
        "name medium-com\ndomains medium.com\n".to_string(),
    ));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteSaved(Err(SaveRefusal::Covered {
            domain: "medium.com".to_string(),
            owner: "medium".to_string()
        }))
    );
}

#[test]
fn a_switch_is_written_to_off_list_and_the_reloaded_list_shows_it_off() {
    let folder = TempFolder::new("switch");
    let keeper = Keeper::new().unwrap();
    let (mut driver, events, _) = driver(&keeper, folder.folders());
    driver.run(Effect::SetSwitch {
        name: "medium".to_string(),
        on: false,
    });
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SwitchSaved(Ok(("medium".to_string(), false)))
    );
    assert_eq!(
        fs::read_to_string(folder.app().join("off.list")).unwrap(),
        "medium\n"
    );
    driver.run(Effect::LoadSites);
    assert_eq!(
        listed(events.recv_timeout(WAIT).unwrap()),
        [
            ("steam".to_string(), SiteKind::BuiltIn, true),
            ("medium".to_string(), SiteKind::BuiltIn, false),
        ]
    );
}

#[test]
fn the_activity_file_goes_to_the_desktop_of_the_given_home_folder() {
    let folder = TempFolder::new("log");
    fs::create_dir_all(folder.root.join("home").join("Desktop")).unwrap();
    let keeper = Keeper::new().unwrap();
    let (mut driver, events, _) = driver(&keeper, folder.folders());
    driver.run(Effect::SaveLog(vec!["starting snisplit".to_string()]));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::LogSaved(Ok(folder
            .root
            .join("home")
            .join("Desktop")
            .join("snisplit-log.txt")))
    );
    assert_eq!(
        fs::read_to_string(
            folder
                .root
                .join("home")
                .join("Desktop")
                .join("snisplit-log.txt")
        )
        .unwrap(),
        "starting snisplit\r\n"
    );
}

#[test]
fn without_a_site_folder_changes_are_refused_and_only_built_ins_are_listed() {
    let keeper = Keeper::new().unwrap();
    let (mut driver, events, _) = driver(&keeper, Folders::default());
    driver.run(Effect::SaveSite(NEWS_PROFILE.to_string()));
    let refused = "no site folder: the APPDATA environment variable is not set".to_string();
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteSaved(Err(SaveRefusal::Other(refused.clone())))
    );
    driver.run(Effect::DeleteSite("news".to_string()));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteRemoved(Err(refused.clone()))
    );
    driver.run(Effect::SetSwitch {
        name: "steam".to_string(),
        on: false,
    });
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SwitchSaved(Err(refused))
    );
    driver.run(Effect::LoadSites);
    assert_eq!(listed(events.recv_timeout(WAIT).unwrap()).len(), 2);
}

#[cfg(windows)]
#[test]
fn the_instance_lock_stays_with_the_keeper_after_the_window_side_is_gone_until_it_finishes() {
    let folder = TempFolder::new("lock");
    let first = Keeper::new().unwrap();
    let (mut first_window, first_events, _) = driver(&first, folder.folders());
    first_window.run(Effect::ClaimInstance);
    assert_eq!(
        first_events.recv_timeout(WAIT).unwrap(),
        Event::InstanceClaimed(Claimed::Ours)
    );
    drop(first_window);
    drop(first_events);
    let second = Keeper::new().unwrap();
    let (mut second_window, second_events, _) = driver(&second, folder.folders());
    second_window.run(Effect::ClaimInstance);
    assert_eq!(
        second_events.recv_timeout(WAIT).unwrap(),
        Event::InstanceClaimed(Claimed::Elsewhere)
    );
    assert_eq!(first.finish(), snisplit_gui::driver::Ending::Clean);
    second_window.run(Effect::ClaimInstance);
    assert_eq!(
        second_events.recv_timeout(WAIT).unwrap(),
        Event::InstanceClaimed(Claimed::Ours)
    );
}

#[test]
fn without_a_site_folder_the_window_runs_unlocked_and_says_why() {
    let keeper = Keeper::new().unwrap();
    let (mut window, events, _) = driver(&keeper, Folders::default());
    window.run(Effect::ClaimInstance);
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::InstanceClaimed(Claimed::Unlocked(
            "no per-user snisplit folder: the APPDATA environment variable is not set".to_string()
        ))
    );
}

#[test]
fn deleting_a_broken_file_named_like_a_built_in_removes_the_file_and_the_built_in_stays() {
    let folder = TempFolder::new("broken-steam");
    fs::create_dir_all(folder.sites()).unwrap();
    fs::write(
        folder.sites().join("steam.profile"),
        "name steam\nnot a key\n",
    )
    .unwrap();
    let keeper = Keeper::new().unwrap();
    let (mut window, events, _) = driver(&keeper, folder.folders());
    window.run(Effect::LoadSites);
    let before = listed(events.recv_timeout(WAIT).unwrap());
    assert_eq!(before.len(), 3);
    assert!(matches!(before[2], (ref name, SiteKind::Broken(_), false) if name == "steam"));
    window.run(Effect::DeleteSite("steam".to_string()));
    assert_eq!(
        events.recv_timeout(WAIT).unwrap(),
        Event::SiteRemoved(Ok("steam".to_string()))
    );
    assert!(!folder.sites().join("steam.profile").exists());
    window.run(Effect::LoadSites);
    assert_eq!(
        listed(events.recv_timeout(WAIT).unwrap()),
        [
            ("steam".to_string(), SiteKind::BuiltIn, true),
            ("medium".to_string(), SiteKind::BuiltIn, true),
        ]
    );
}
