use super::*;
use std::process;

struct TempFolder(PathBuf);

impl TempFolder {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("snisplit-layout-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp folder");
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn installed_files_live_in_a_snisplit_folder_under_program_files() {
    let layout = Layout::under(Path::new(r"C:\Program Files"));
    assert_eq!(layout.root(), Path::new(r"C:\Program Files\snisplit"));
    assert_eq!(
        layout.executable(),
        PathBuf::from(r"C:\Program Files\snisplit\snisplit.exe")
    );
    assert_eq!(
        layout.profiles(),
        PathBuf::from(r"C:\Program Files\snisplit\profiles")
    );
    assert_eq!(
        layout.profile(1),
        PathBuf::from(r"C:\Program Files\snisplit\profiles\1.profile")
    );
    assert_eq!(
        layout.profile(12),
        PathBuf::from(r"C:\Program Files\snisplit\profiles\12.profile")
    );
    assert_eq!(
        layout.log_file(),
        PathBuf::from(r"C:\Program Files\snisplit\logs\snisplit.log")
    );
    assert_eq!(
        layout.old_log_file(),
        PathBuf::from(r"C:\Program Files\snisplit\logs\snisplit.old.log")
    );
}

#[test]
fn a_file_is_inside_its_folder_whatever_the_spelling_of_the_folder() {
    let temp = TempFolder::new("inside");
    let folder = temp.0.join("snisplit");
    fs::create_dir_all(folder.join("sub")).expect("create folders");
    let file = folder.join("sub").join("snisplit.exe");
    fs::write(&file, b"exe").expect("write file");
    let upper = temp.0.join("SNISPLIT");

    assert!(is_inside(&file, &folder));
    assert!(is_inside(&file, &upper));
    assert!(is_inside(&folder.join(r"sub\..\sub\snisplit.exe"), &folder));
}

#[test]
fn the_same_file_is_recognised_through_another_spelling_and_a_missing_copy_is_not_it() {
    let temp = TempFolder::new("same");
    let file = temp.0.join("snisplit.exe");
    fs::write(&file, b"exe").expect("write file");
    let other = temp.0.join("other.exe");
    fs::write(&other, b"exe").expect("write other");

    assert!(is_same_file(&file, &temp.0.join("SNISPLIT.EXE")));
    assert!(is_same_file(&file, &temp.0.join(r".\snisplit.exe")));
    assert!(!is_same_file(&file, &other));
    assert!(!is_same_file(&file, &temp.0.join("missing.exe")));
}

#[test]
fn a_file_in_a_sibling_folder_with_the_same_prefix_is_not_inside() {
    let temp = TempFolder::new("sibling");
    let folder = temp.0.join("snisplit");
    let sibling = temp.0.join("snisplit2");
    fs::create_dir_all(&folder).expect("create folder");
    fs::create_dir_all(&sibling).expect("create sibling");
    let file = sibling.join("snisplit.exe");
    fs::write(&file, b"exe").expect("write file");

    assert!(!is_inside(&file, &folder));
    assert!(!is_inside(&folder.join("missing.exe"), &folder));
}
