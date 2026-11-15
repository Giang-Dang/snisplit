use std::path::PathBuf;

use super::*;

struct TempFolder(PathBuf);

impl TempFolder {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("snisplit-instance-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn app(&self) -> PathBuf {
        self.0.join("snisplit")
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::metadata(self.app().join("instance.lock")) {
            let mut permissions = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(self.app().join("instance.lock"), permissions);
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
#[test]
fn a_second_claim_is_refused_while_the_first_is_held_and_succeeds_once_it_is_dropped() {
    let folder = TempFolder::new("second");
    let first = claim(Some(&folder.app()));
    assert!(matches!(first, Claim::Ours(_)), "{first:?}");
    let second = claim(Some(&folder.app()));
    assert!(matches!(second, Claim::Elsewhere), "{second:?}");
    drop(first);
    let third = claim(Some(&folder.app()));
    assert!(matches!(third, Claim::Ours(_)), "{third:?}");
}

#[cfg(windows)]
#[test]
fn a_lock_file_that_cannot_be_written_is_still_claimed_and_still_excludes_a_second_claim() {
    let folder = TempFolder::new("read-only");
    fs::create_dir_all(folder.app()).unwrap();
    let file = folder.app().join("instance.lock");
    fs::write(&file, b"made by another account").unwrap();
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();
    assert!(OpenOptions::new().write(true).open(&file).is_err());
    let first = claim(Some(&folder.app()));
    assert!(matches!(first, Claim::Ours(_)), "{first:?}");
    let second = claim(Some(&folder.app()));
    assert!(matches!(second, Claim::Elsewhere), "{second:?}");
}

#[test]
fn a_lock_file_left_behind_by_a_killed_process_does_not_block() {
    let folder = TempFolder::new("stale");
    fs::create_dir_all(folder.app()).unwrap();
    fs::write(
        folder.app().join("instance.lock"),
        b"left by a killed process",
    )
    .unwrap();
    let claimed = claim(Some(&folder.app()));
    assert!(matches!(claimed, Claim::Ours(_)), "{claimed:?}");
}

#[test]
fn a_missing_folder_is_created_and_claimed() {
    let folder = TempFolder::new("missing");
    let claimed = claim(Some(&folder.app().join("nested")));
    assert!(matches!(claimed, Claim::Ours(_)), "{claimed:?}");
    assert!(folder.app().join("nested").join("instance.lock").is_file());
}

#[test]
fn a_folder_that_cannot_be_created_proceeds_unlocked_rather_than_held() {
    let folder = TempFolder::new("blocked");
    fs::create_dir_all(&folder.0).unwrap();
    let in_the_way = folder.0.join("snisplit");
    fs::write(&in_the_way, b"a file where the folder should be").unwrap();
    let claimed = claim(Some(&in_the_way));
    assert!(matches!(claimed, Claim::Unlocked(_)), "{claimed:?}");
}

fn refusal(claimed: Claim) -> String {
    match claimed {
        Claim::Unlocked(error) => error.to_string(),
        other => panic!("expected an unlocked claim, got {other:?}"),
    }
}

#[test]
fn a_lock_path_that_is_a_folder_is_refused_and_leaves_the_claim_unlocked() {
    let folder = TempFolder::new("lock-is-folder");
    fs::create_dir_all(folder.app().join("instance.lock")).unwrap();
    assert_eq!(
        refusal(claim(Some(&folder.app()))),
        "the snisplit lock file is a link or not a regular file, so it is not used"
    );
}

#[cfg(windows)]
#[test]
fn a_lock_folder_that_is_a_junction_is_refused_and_nothing_is_created_in_its_target() {
    let folder = TempFolder::new("junction");
    let target = folder.0.join("target");
    fs::create_dir_all(&target).unwrap();
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(folder.app())
        .arg(&target)
        .output()
        .unwrap();
    if !made.status.success() {
        eprintln!("skipped: mklink /J failed: {made:?}");
        return;
    }
    assert_eq!(
        refusal(claim(Some(&folder.app()))),
        "the snisplit lock folder is a link or not a real folder, so it is not used"
    );
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
}

#[cfg(windows)]
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;

#[cfg(windows)]
#[test]
fn a_lock_folder_that_is_a_symbolic_link_is_refused_and_nothing_is_created_in_its_target() {
    let folder = TempFolder::new("symlink-folder");
    let target = folder.0.join("target");
    fs::create_dir_all(&target).unwrap();
    match std::os::windows::fs::symlink_dir(&target, folder.app()) {
        Err(error) if error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD) => {
            eprintln!("skipped: creating a symbolic link needs a privilege this process lacks");
            return;
        }
        made => made.unwrap(),
    }
    assert_eq!(
        refusal(claim(Some(&folder.app()))),
        "the snisplit lock folder is a link or not a real folder, so it is not used"
    );
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn a_lock_file_that_is_a_symbolic_link_is_refused_and_its_target_is_left_alone() {
    let folder = TempFolder::new("symlink-file");
    fs::create_dir_all(folder.app()).unwrap();
    let target = folder.0.join("elsewhere.txt");
    fs::write(&target, b"not snisplit's").unwrap();
    match std::os::windows::fs::symlink_file(&target, folder.app().join("instance.lock")) {
        Err(error) if error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD) => {
            eprintln!("skipped: creating a symbolic link needs a privilege this process lacks");
            return;
        }
        made => made.unwrap(),
    }
    assert_eq!(
        refusal(claim(Some(&folder.app()))),
        "the snisplit lock file is a link or not a regular file, so it is not used"
    );
    assert_eq!(fs::read(&target).unwrap(), b"not snisplit's");
}

#[test]
fn no_machine_wide_folder_proceeds_unlocked_and_says_why() {
    match claim(None) {
        Claim::Unlocked(error) => assert_eq!(
            error.to_string(),
            "no machine-wide snisplit folder: the ProgramData environment variable is not set"
        ),
        other => panic!("expected an unlocked claim, got {other:?}"),
    }
}

#[cfg(windows)]
#[test]
fn the_lock_lives_in_the_snisplit_folder_under_program_data() {
    let folder = folder_from(|key| (key == "ProgramData").then(|| OsString::from(r"D:\Shared")));
    assert_eq!(
        folder.as_deref().map(lock_path),
        Some(PathBuf::from(r"D:\Shared\snisplit\instance.lock"))
    );
}

#[test]
fn an_unset_or_empty_program_data_gives_no_folder() {
    assert_eq!(folder_from(|_| None), None);
    assert_eq!(folder_from(|_| Some(OsString::new())), None);
}
