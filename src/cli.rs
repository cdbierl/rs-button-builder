//! Parses command-line arguments and provides the application's usage text.
use std::path::PathBuf;

/// Command-line usage and options printed by the help action.
pub const USAGE: &str = "Usage: rs_button_builder [OPTIONS] [CONFIG]

Launch a configurable command button grid with an interactive terminal.
CONFIG is a local YAML file (default: ./buttons.yaml).

Options:
  -h, --help                 Show this help
";
/// The action selected from the command-line arguments.
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

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            _ if !arg.starts_with('-') && config.is_none() => config = Some(arg.into()),
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(Action::Run { config })
}
