use super::*;
use crate::args::{self, Command, ServiceCommand};
use std::net::Ipv4Addr;
use std::path::PathBuf;

const INSTALLED_EXE: &str = r"C:\Program Files\snisplit\snisplit.exe";

fn every_option() -> RunOptions {
    RunOptions {
        verbose: true,
        profiles: vec!["medium", "steam"],
        profile_files: vec![
            PathBuf::from(r"C:\Program Files\snisplit\profiles\1.profile"),
            PathBuf::from(r"C:\Program Files\snisplit\profiles\2.profile"),
        ],
        routing: true,
        upstreams: vec![Ipv4Addr::new(9, 9, 9, 9), Ipv4Addr::new(1, 0, 0, 1)],
    }
}

#[test]
fn plain_arguments_are_written_as_they_are() {
    assert_eq!(quote("medium"), "medium");
    assert_eq!(quote("9.9.9.9"), "9.9.9.9");
    assert_eq!(quote(r"C:\snisplit\1.profile"), r"C:\snisplit\1.profile");
}

#[test]
fn arguments_with_spaces_or_tabs_or_nothing_are_quoted() {
    assert_eq!(
        quote(r"C:\Program Files\snisplit\profiles\1.profile"),
        r#""C:\Program Files\snisplit\profiles\1.profile""#
    );
    assert_eq!(quote("a\tb"), "\"a\tb\"");
    assert_eq!(quote(""), r#""""#);
}

#[test]
fn backslashes_are_doubled_only_before_a_quote_or_the_closing_quote() {
    assert_eq!(quote(r"C:\a b\"), r#""C:\a b\\""#);
    assert_eq!(quote(r"C:\a b\\"), r#""C:\a b\\\\""#);
    assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
    assert_eq!(quote(r#"a\"b"#), r#""a\\\"b""#);
    assert_eq!(quote(r"a\\b c"), r#""a\\b c""#);
}

#[test]
fn the_registered_command_line_quotes_the_executable_and_lists_every_option() {
    let options = RunOptions {
        verbose: true,
        profiles: vec!["medium"],
        profile_files: vec![PathBuf::from(
            r"C:\Program Files\snisplit\profiles\1.profile",
        )],
        routing: true,
        upstreams: vec![Ipv4Addr::new(9, 9, 9, 9)],
    };
    assert_eq!(
        command_line(INSTALLED_EXE, &service_arguments(&options)),
        r#""C:\Program Files\snisplit\snisplit.exe" service run --profile medium --profile-file "C:\Program Files\snisplit\profiles\1.profile" --upstream 9.9.9.9 --verbose"#
    );
}

#[test]
fn an_executable_path_without_spaces_is_still_quoted() {
    let options = RunOptions {
        verbose: false,
        profiles: vec!["steam", "medium"],
        profile_files: vec![],
        routing: true,
        upstreams: vec![],
    };
    assert_eq!(
        command_line(
            r"D:\Apps\snisplit\snisplit.exe",
            &service_arguments(&options)
        ),
        r#""D:\Apps\snisplit\snisplit.exe" service run --profile steam --profile medium"#
    );
}

#[test]
fn the_service_arguments_parse_back_to_the_same_service_run_options() {
    let options = every_option();
    assert_eq!(
        args::parse(service_arguments(&options)),
        Ok(Command::Service(ServiceCommand::Run(options)))
    );
}
