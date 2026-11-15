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
