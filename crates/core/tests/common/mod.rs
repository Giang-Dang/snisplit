use std::fs;
use std::path::PathBuf;

const STREAM_EXTENSION: &str = "bin";

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("client_hello")
}

pub fn fixture_stems() -> Vec<String> {
    let entries = fs::read_dir(fixture_directory()).expect("fixture directory is readable");
    let mut stems: Vec<String> = entries
        .map(|entry| entry.expect("directory entry is readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == STREAM_EXTENSION))
        .filter_map(|path| path.file_stem()?.to_str().map(str::to_owned))
        .collect();
    stems.sort();
    stems
}

pub fn read_fixture_file(stem: &str, extension: &str) -> Vec<u8> {
    let path = fixture_directory().join(format!("{stem}.{extension}"));
    fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

pub fn read_stream(stem: &str) -> Vec<u8> {
    read_fixture_file(stem, STREAM_EXTENSION)
}
