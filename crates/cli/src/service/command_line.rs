use crate::args::RunOptions;

const NEEDS_QUOTES: [char; 4] = [' ', '\t', '\n', '"'];

pub fn service_arguments(options: &RunOptions) -> Vec<String> {
    let mut arguments = vec!["service".to_string(), "run".to_string()];
    for name in &options.profiles {
        arguments.extend(["--profile".to_string(), (*name).to_string()]);
    }
    for path in &options.profile_files {
        arguments.extend([
            "--profile-file".to_string(),
            path.to_string_lossy().into_owned(),
        ]);
    }
    for address in &options.upstreams {
        arguments.extend(["--upstream".to_string(), address.to_string()]);
    }
    if options.verbose {
        arguments.push("--verbose".to_string());
    }
    arguments
}

pub fn command_line(executable: &str, arguments: &[String]) -> String {
    let mut line = format!("\"{executable}\"");
    for argument in arguments {
        line.push(' ');
        line.push_str(&quote(argument));
    }
    line
}

pub fn quote(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains(NEEDS_QUOTES) {
        return argument.to_string();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(2 * backslashes + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(character);
                backslashes = 0;
            }
        }
    }
    quoted.push_str(&"\\".repeat(2 * backslashes));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests;
