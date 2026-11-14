use super::*;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process;

const ALPHA: &str = "name alpha\ndomains alpha.example\ntls-ports 443\ntls-split 1\n";
const BETA: &str = "name beta\ndomains beta.example\ntls-ports 443\ntls-split 1\n";
const BETA_WITH_ALPHA_DOMAIN: &str =
    "name beta\ndomains alpha.example\ntls-ports 443\ntls-split 1\n";

struct TempFolder(PathBuf);

impl TempFolder {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("snisplit-control-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp folder");
        Self(path)
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).expect("write file");
        path
    }

    fn layout(&self) -> Layout {
        Layout::under(&self.0.join("Program Files"))
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn options_with_files(files: Vec<PathBuf>) -> RunOptions {
    RunOptions {
        verbose: true,
        profiles: vec!["medium"],
        profile_files: files,
        routing: true,
        upstreams: vec![Ipv4Addr::new(9, 9, 9, 9)],
    }
}

#[test]
fn good_profile_files_pass_the_install_check() {
    let temp = TempFolder::new("good");
    let files = vec![temp.file("a.profile", ALPHA), temp.file("b.profile", BETA)];
    assert!(check_profiles(&options_with_files(files)).is_ok());
}

#[test]
fn a_bad_or_missing_profile_file_fails_the_install_check_and_names_the_file() {
    let temp = TempFolder::new("bad");
    let bad = temp.file("bad.profile", "name broken\n");
    let error = check_profiles(&options_with_files(vec![bad.clone()])).expect_err("bad file");
    assert!(
        error.to_string().contains(&bad.display().to_string()),
        "{error}"
    );

    let missing = temp.0.join("missing.profile");
    let error = check_profiles(&options_with_files(vec![missing.clone()])).expect_err("missing");
    assert!(
        error.to_string().contains(&missing.display().to_string()),
        "{error}"
    );
}

#[test]
fn profiles_that_claim_the_same_domain_fail_the_install_check() {
    let temp = TempFolder::new("clash");
    let files = vec![
        temp.file("a.profile", ALPHA),
        temp.file("b.profile", BETA_WITH_ALPHA_DOMAIN),
    ];
    let error = check_profiles(&options_with_files(files)).expect_err("clash");
    assert_eq!(
        error.to_string(),
        "domain alpha.example is in both profile alpha and profile beta"
    );
}

#[test]
fn install_copies_the_executable_and_numbers_the_profiles_in_command_line_order() {
    let temp = TempFolder::new("copy");
    let executable = temp.file("snisplit.exe", "the executable");
    let options = options_with_files(vec![
        temp.file("second-named-b.profile", BETA),
        temp.file("a.profile", ALPHA),
    ]);
    let layout = temp.layout();

    let installed = copy_files(&layout, &options, &executable).expect("copy");

    let root = temp.0.join("Program Files").join("snisplit");
    let first = root.join("profiles").join("1.profile");
    let second = root.join("profiles").join("2.profile");
    assert_eq!(
        installed,
        RunOptions {
            verbose: true,
            profiles: vec!["medium"],
            profile_files: vec![first.clone(), second.clone()],
            routing: true,
            upstreams: vec![Ipv4Addr::new(9, 9, 9, 9)],
        }
    );
    assert_eq!(
        fs::read_to_string(root.join("snisplit.exe")).expect("read exe"),
        "the executable"
    );
    assert_eq!(fs::read_to_string(first).expect("read 1"), BETA);
    assert_eq!(fs::read_to_string(second).expect("read 2"), ALPHA);
}

#[test]
fn install_drops_profile_copies_left_by_an_earlier_install() {
    let temp = TempFolder::new("stale");
    let executable = temp.file("snisplit.exe", "the executable");
    let layout = temp.layout();
    let stale = temp.0.join(r"Program Files\snisplit\profiles\2.profile");
    fs::create_dir_all(stale.parent().expect("profiles")).expect("create profiles");
    fs::write(&stale, BETA).expect("write stale copy");

    copy_files(
        &layout,
        &options_with_files(vec![temp.file("a.profile", ALPHA)]),
        &executable,
    )
    .expect("copy");

    assert!(!stale.exists());
    let names: Vec<_> = fs::read_dir(temp.0.join(r"Program Files\snisplit\profiles"))
        .expect("list profiles")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(names, vec!["1.profile"]);
}

#[test]
fn install_from_the_installed_executable_leaves_it_in_place() {
    let temp = TempFolder::new("self");
    let layout = temp.layout();
    let installed = temp.0.join(r"Program Files\snisplit\snisplit.exe");
    fs::create_dir_all(installed.parent().expect("root")).expect("create root");
    fs::write(&installed, "the installed executable").expect("write exe");

    copy_files(&layout, &options_with_files(vec![]), &installed).expect("copy");

    assert_eq!(
        fs::read_to_string(&installed).expect("read exe"),
        "the installed executable"
    );
}

#[test]
fn uninstall_removes_the_install_folder_with_everything_in_it() {
    let temp = TempFolder::new("remove");
    let layout = temp.layout();
    let root = temp.0.join(r"Program Files\snisplit");
    fs::create_dir_all(root.join("logs")).expect("create logs");
    fs::write(root.join(r"logs\snisplit.log"), "log").expect("write log");
    fs::write(root.join("snisplit.exe"), "exe").expect("write exe");
    let running = temp.file("elsewhere.exe", "exe");

    assert_eq!(
        remove_files(&layout, &running).expect("remove"),
        Removal::Removed
    );
    assert!(!root.exists());
    assert_eq!(
        remove_files(&layout, &running).expect("remove again"),
        Removal::NothingThere
    );
}

#[test]
fn uninstall_leaves_the_install_folder_when_it_runs_from_it() {
    let temp = TempFolder::new("in-use");
    let layout = temp.layout();
    let running = temp.0.join(r"Program Files\snisplit\snisplit.exe");
    fs::create_dir_all(running.parent().expect("root")).expect("create root");
    fs::write(&running, "exe").expect("write exe");

    assert_eq!(
        remove_files(&layout, &running).expect("remove"),
        Removal::LeftInUse
    );
    assert!(running.exists());
}

#[test]
fn recovery_restarts_after_five_five_and_thirty_seconds_and_resets_after_a_day() {
    let actions: Vec<(i32, u32)> = restart_actions()
        .iter()
        .map(|action| (action.Type, action.Delay))
        .collect();
    assert_eq!(actions, vec![(1, 5_000), (1, 5_000), (1, 30_000)]);
    assert_eq!(reset_period_seconds(), 86_400);
}

#[test]
fn access_denied_becomes_a_request_for_an_administrator_terminal() {
    let error = needs_administrator(io::Error::from_raw_os_error(5), "install");
    assert_eq!(
        error.to_string(),
        "snisplit service install needs an administrator terminal"
    );
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

    let other = needs_administrator(io::Error::from_raw_os_error(1060), "install");
    assert_eq!(other.raw_os_error(), Some(1060));
}

#[test]
fn an_existing_service_is_reported_as_uninstall_first() {
    let error = creation_failure(io::Error::from_raw_os_error(1073));
    assert_eq!(
        error.to_string(),
        "service snisplit already exists; run snisplit service uninstall first"
    );
    let marked = creation_failure(io::Error::from_raw_os_error(1072));
    assert!(
        marked.to_string().contains("still being removed"),
        "{marked}"
    );
}
