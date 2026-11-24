# Button Builder

A Linux desktop app for launching commands from a configurable button grid,
with interactive terminal tabs. Built with Rust 2024, egui, and egui-material3.
The Cargo package and executable are named `rs_button_builder` (version `0.1.0`).

## Features

- Named YAML layouts with configurable grid dimensions, button labels, actions, and colors.
- Multiple process terminals with independent output, scrollback, fonts, and colors.
- Interactive commands, shell pipelines, and log following through Unix pseudo-terminals.
- Optional per-run output logs that retain output beyond the scrollback buffer.
- Material controls with light/dark themes, five accent colors, and locally loaded fonts.

## Requirements

- Linux 5.3 or newer with X11 or Wayland and an OpenGL-capable graphics driver.
- Rust 1.88 or newer, Cargo, and a system C toolchain/linker to build from source.
- The executables used by your buttons and permission to read their input files.

## Dependencies

Direct Rust dependencies from [Cargo.toml](Cargo.toml):

| Dependency | Version requirement | Purpose |
| --- | --- | --- |
| `eframe` | `0.33` | Native window and OpenGL rendering, with X11 and Wayland support. |
| `egui` | `0.33.0` | GUI layout, widgets, and input handling. |
| `egui-material3` | `0.0.9` | Material controls and theme colors. |
| `serde` | `1.0` | Configuration serialization and deserialization. |
| `serde_yaml_ng` | `0.10` | YAML layout parsing and writing. |
| `libc` | `0.2` | Unix pseudo-terminals, process groups, and terminal sizing. |
| `shlex` | `2.0` | Parsing quoted program arguments. |
| `fontdb` | `0.23` | Discovering installed fonts and Fontconfig directories. |
| `ttf-parser` | `0.25` | Inspecting font glyph widths. |
| `unicode-width` | `0.2` | Unicode character widths for terminal cells. |
| `log` | `0.4` | Application logging. |
| `env_logger` | `0.11` | Logging configuration through `RUST_LOG`. |
| `dotenvy` | `0.15` | Loading environment variables from `.env`. |

Fonts and theme assets are loaded locally. Optional asset downloads in
`egui-material3` are disabled.

## Build and run

From the project directory:

```sh
cargo build --locked --release
./target/release/rs_button_builder
./target/release/rs_button_builder --file my-buttons.yaml
```

For development, Cargo can build and launch the GUI directly:

```sh
cargo run --locked
cargo run --locked -- -f my-buttons.yaml
cargo run --locked -- --help
```

`--help` prints command-line usage. Use `-f <filepath>` or `--file <filepath>` to
select a YAML layout file. Dependency versions are recorded in [Cargo.lock](Cargo.lock);
`--locked` preserves those versions.

## Usage

- **Configure and arrange buttons:** click an empty slot or right-click a button to
  edit it. Drag buttons to swap positions; right-click and choose **Reset** to clear
  one. Use **Edit layout** to rename the layout or change its rows and columns. The
  footer apps button collapses the button panel to make more room for terminals.
- **Run commands:** click a configured button to start it or focus its running tab.
  Ctrl+click starts another instance. Buttons support direct programs, `/bin/sh`
  scripts, working directories, colors, auto start, logging, and close on success.
- **Use terminals:** click a terminal to type. The toolbar controls appearance,
  interrupt, stop, close, jump to live output, pause display updates, copy, and line
  wrapping. Drag selected text to copy it automatically. Ctrl+mouse wheel changes
  the terminal font size.
- **Split terminals:** right-click a tab and choose **Split right** or **Split below**.
  Panes can be split repeatedly in either direction. Drag dividers to resize them,
  move tabs between panes, or choose **Merge all panes**. Pane layouts are not saved.
- **Save and load layouts:** open the footer menu for new, recent, load, edit, save,
  save as, and exit actions. An asterisk marks unsaved changes in the title and menu.
- **Change the theme:** use the footer palette for Light, Dark, System, and five
  accent colors. Theme changes apply to existing terminals unless they use explicit
  custom colors.

Successful commands with no meaningful output close automatically. **Close on
success** closes any command that exits with status 0. Failures, interrupted or
stopped commands, and terminal or logging errors remain open.

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| Ctrl+N | New layout (asks about unsaved changes) |
| Ctrl+O | Load config |
| Ctrl+E | Toggle layout editing (does not save changes) |
| Ctrl+B | Show or hide the button panel |
| Ctrl+S | Save layout |
| Ctrl+Shift+S | Save layout as |
| Ctrl+Page Up / Page Down | Previous / next terminal tab in the focused pane, wrapping at either end |
| Ctrl+Shift+W | Close the active terminal tab and stop its process |
| Ctrl+Shift+Q | Exit |
| Ctrl+Shift+C | Copy selected terminal text, or all output |
| Ctrl+Shift+V | Paste into the focused terminal |
| Ctrl+C | Interrupt the foreground terminal job |

Layout and tab shortcuts work while the terminal is focused and are reserved by
the app. They are disabled while a dialog is open. Holding a shortcut does not
repeat its action. Ctrl+W and Ctrl+Q remain available to terminal programs.

## Configuration and examples

Without `--file`, the app reopens the last loaded or saved layout. Settings are
stored in `$XDG_CONFIG_HOME/rs_button_builder/settings.yaml`, or
`~/.config/rs_button_builder/settings.yaml`. They contain the theme, last layout,
and recent layouts. Button layouts remain separate YAML files.

On first launch, the app opens an empty layout.
**Load config…** selects an existing layout. Save actions reuse the selected file;
**Save layout as…** chooses another. Unsaved changes are not saved on exit.

A minimal layout:

```yaml
display_name: System Monitor
rows: 1
columns: 1
buttons:
  - label: Run Top
    command: /usr/bin/top
    arguments: -d 1
```

See [example layouts](examples/README.md) for more configurations. Button and
terminal colors use optional `[r, g, b]` arrays in `fg_color` and `bg_color`.
Omitting terminal colors makes them follow the current theme.

### Button run options

- `auto_start: true` starts the button when its layout loads.
- `close_on_success: true` closes its terminal after exit status 0.
- `log_output: true` writes its output to a per-run text log.

These options are also available in the button editor and apply to new runs.

### Terminal output logs

Logs default to `~/Documents/ButtonBuilder/logs`. Set
`BUTTON_BUILDER_LOG_DIR` in the environment or `.env` to override the location:

```dotenv
BUTTON_BUILDER_LOG_DIR=./logs
```

Logs are created only after meaningful text appears. Each run receives a timestamped
file under a directory derived from the button label. Logs contain plain UTF-8 text;
ANSI formatting and terminal controls are removed. Logging runs off the UI thread
and continues while pane updates are paused. Hover over terminal status text to see
its log path or an error.

## Development

| Source | Responsibility |
| --- | --- |
| `src/main.rs`, `src/cli.rs` | Startup, arguments, logging, and `.env` loading. |
| `src/app.rs` | Application state, layout persistence, notices, and frame composition. |
| `src/workspace/mod.rs` | Terminal tabs, pane membership, selection, process lifecycle, and repaint scheduling. |
| `src/workspace/panes.rs` | Nested pane layout, divider proportions, and collapsing empty sections. |
| `src/ui/` | Footer menu, button grid/editor, layout dialogs, appearance, terminal rendering, and shared styling. |
| `src/app/tests/` | Layout, button, terminal, pane, and theme tests with shared UI interaction helpers. |
| `src/layout_config.rs` | YAML schema, validation, loading, and saving. |
| `src/settings.rs` | App preferences and startup layout restoration. |
| `src/theme.rs` | UI appearance modes, accent colors, and terminal theme defaults. |
| `src/terminal/` | Pseudo-terminal processes, input, and screen rendering. |
| `src/terminal/logging/` | Streaming conversion of terminal output to plain-text logs. |
| `src/terminal/tests.rs` | Terminal process, output, and logging tests. |
| `src/fonts.rs` | Bundled icons and local font discovery. |

Run the checks before submitting changes:

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```

The repository's Cargo configuration runs tests serially: UI tests share a global
Material theme, and process responsiveness tests include elapsed-time assertions.

Tests use local Linux tools, including `/usr/bin/top`, `/usr/bin/jq`, and
`/usr/bin/tput`, and generated input. They cover stalled input, ignored interrupts,
raw terminal mode, continuous output, and cleanup of background jobs. The log-follow
test uses a temporary log file.
When `/usr/bin/htop` is installed, tests also check its colors, resizing, and
keyboard quit.
Keep changes focused.

Console logging uses `RUST_LOG` (default: `info`). Process lifecycle and logging
events use `info`; `RUST_LOG=debug` also shows tab, pane, and display actions.
Startup loads an optional `.env` file; child processes inherit the app's environment.

## Known limitations

Linux is the supported platform. The embedded terminal uses `TERM=xterm-256color`
and supports ANSI colors (including 256-color and RGB sequences), Unicode, and
alternate screens. Terminal graphics, mouse reporting, and full text reflow on
width changes are not implemented.
Session cleanup uses Linux pidfd APIs to avoid targeting reused process IDs.
Jobs that detach into a new session and processes running on remote hosts can
require separate cleanup. A process stuck in uninterruptible kernel I/O can take
time to exit after `SIGKILL`; the UI does not wait for it.

## License

An application license has not yet been declared in [Cargo.toml](Cargo.toml).
The [bundled Material Symbols font](assets/README.md) is distributed under
[Apache License 2.0](licenses/Apache-2.0.txt); that license applies to the font asset.
