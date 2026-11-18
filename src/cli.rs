//! Parses command-line arguments and provides the application's usage text.
use std::path::PathBuf;

/// Command-line usage and options printed by the help action.
pub const USAGE: &str = "Usage: rs_button_builder [OPTIONS]

Launch a configurable command button grid with an interactive terminal.
Without a config file, start with an empty layout
and load a config or choose a file when saving in the GUI.

Options:
  -f, --file <FILEPATH>      Load a local YAML configuration file
  -h, --help                 Show this help
";
/// The action selected from the command-line arguments.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Print the usage text and exit successfully.
    Help,
    Run {
        /// The configuration value supplied on the command line.
        config: Option<PathBuf>,
    },
}

/// Parses supported options and returns the requested action or an error message.
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Action, String> {
    let mut config = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-f" | "--file" => {
                if config.is_some() {
                    return Err("configuration file was specified more than once".into());
                }
                let path = args
                    .next()
                    .filter(|path| !path.trim().is_empty() && !path.starts_with('-'))
                    .ok_or_else(|| format!("{arg} requires a file path"))?;
                config = Some(path.into());
            }
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(Action::Run { config })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Action, String> {
        parse_args(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn file_flags_select_the_supplied_path() {
        for flag in ["-f", "--file"] {
            for path in [
                "examples/system-monitor.yaml",
                "/tmp/My layouts/targeting.yaml",
            ] {
                assert_eq!(
                    parse(&[flag, path]).unwrap(),
                    Action::Run {
                        config: Some(path.into())
                    }
                );
            }
        }
    }

    #[test]
    fn no_file_and_help_are_supported() {
        assert_eq!(parse(&[]).unwrap(), Action::Run { config: None });
        for flag in ["-h", "--help"] {
            assert_eq!(parse(&[flag]).unwrap(), Action::Help);
        }
    }

    #[test]
    fn file_flags_require_a_path_and_cannot_be_repeated() {
        for flag in ["-f", "--file"] {
            for args in [
                vec![flag],
                vec![flag, ""],
                vec![flag, "--help"],
                vec![flag, "-f"],
            ] {
                assert_eq!(
                    parse(&args).unwrap_err(),
                    format!("{flag} requires a file path")
                );
            }
        }
        for flags in [["-f", "-f"], ["--file", "--file"], ["-f", "--file"]] {
            assert!(parse(&[flags[0], "one.yaml", flags[1], "two.yaml"]).is_err());
        }
    }

    #[test]
    fn positional_paths_and_unknown_options_are_rejected() {
        for args in [
            vec!["buttons.yaml"],
            vec!["--unknown"],
            vec!["-f", "one.yaml", "two.yaml"],
        ] {
            assert!(parse(&args).is_err());
        }
    }
}
