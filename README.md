# Button Builder

A Linux desktop app for launching commands from a configurable button grid,
with interactive terminal tabs. Built with Rust 2024, egui, and egui-material3.
The Cargo package and executable are named `rs_button_builder` (version `0.1.0`).

![Button Builder GUI with a command grid, three terminal tabs, and JSON output](docs/images/button-builder.png)

GUI preview with generated sample output.

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

- **Configure buttons:** right-click a button, click an unassigned button, or enable
  **Edit buttons**. Set its label, executable, arguments, working directory, and colors.
  Empty slots show a muted **new window** icon. **Shell Script (/bin/sh)** runs the complete
  command through `/bin/sh`.
- **Clear a button:** right-click it and choose **Reset** to restore an empty
  slot with its default label and colors. Use **Save layout** to keep the reset.
- **Reposition buttons:** drag a button onto another slot to swap their positions.
  Dragging works with **Edit buttons** on or off. Drop outside the grid or press
  **Escape** to cancel. Use **Save layout** to keep the new positions.
- **Customize layouts:** change **Rows** and **Columns**, or click the heading above
  the grid to rename the layout. The window title shows
  **Button Builder - {display name}**. **New layout** creates an empty 3×3 grid and
  asks how to handle unsaved changes. Existing terminal tabs stay open; the new
  layout's first save asks for a filename.
- **Manage terminals:** click a button to focus its running terminal, or open a new
  tab if it has no running terminal in the current layout. Finished tabs and their
  output are preserved. New tabs use the layout's terminal appearance defaults.
  Running tabs keep collecting output. Click the terminal to
  type; use the tab bar and its arrows to switch sessions.
  Commands that finish successfully without printing meaningful text close their tabs
  automatically and show a brief success notice. Enable **Close on success** for a
  button to also close successful runs that printed output. Failures, interrupted
  or stopped commands, and terminal or logging errors keep their tabs.
- **Copy terminal text:** click and drag to highlight text; releasing the mouse
  automatically copies it to the clipboard. **Copy** or **Ctrl+Shift+C** copies the
  selection, or all output when nothing is selected. Click in the terminal to clear
  the selection. **Ctrl+C** still interrupts the process.
- **Adjust appearance:** **Appearance** changes the selected tab's font and colors
  and the defaults for new tabs. **Use theme colors** clears custom terminal colors
  so the tab follows the footer theme, while keeping its font settings.
  **Ctrl+mouse wheel** over the terminal adjusts its
  font size (8–32 pt); plain scrolling or dragging the right-hand scrollbar moves
  through output history. Drag to the bottom or click **Jump to live** to follow new output.
  **Refresh fonts** discovers installed monospace fonts.
- **Choose a UI theme:** the palette control at the right of the footer opens
  Light, Dark, and System modes plus five accent colors. Changes apply immediately
  and are saved in app settings. Terminals use matching light or dark colors,
  including existing output and scrollback. Explicit custom button and terminal
  colors are preserved. System mode follows the desktop appearance when available.
- **Stop processes:** **Ctrl+C** sends `SIGINT` to the foreground job, allowing it
  to clean up; programs can ignore this signal. **Stop** sends `SIGKILL` and ends
  local jobs in that terminal session. Pending output is discarded; displayed output
  remains. **Close tab** uses the same forceful cleanup. **Exit** closes all tabs.
  Process cleanup runs off the UI thread, so it does not wait for a stalled child.

## Configuration and examples

Without `--file`, the app reopens the last successfully loaded or saved layout.
App settings live in `~/.config/rs_button_builder/settings.yaml`, or
`$XDG_CONFIG_HOME/rs_button_builder/settings.yaml` when that variable contains an
absolute path. Settings store the UI theme and last layout separately from button
layouts. The remembered layout path is absolute, so startup works from another
working directory.

On first launch, the app starts with an
[empty, unsaved grid](docs/images/unsaved-layout.png). A missing or invalid remembered
layout also opens an empty grid and displays an error. An explicit `--file` overrides
the remembered layout; a missing or invalid explicit file reports an error and exits.
Creating a new unsaved layout keeps the last file remembered until the new layout
is saved. Unsaved edits and running processes are not restored.
**Load config…** selects a layout and makes it the destination for subsequent saves.
**Save layout**, **Save button**, and **Save appearance** write to that file;
without a selected file, they open a save dialog. Unsaved changes are not
automatically saved on exit.
When a save filename has no extension, `.yaml` is added automatically.

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

See [example layouts](examples/README.md) for system monitoring, JSON pipelines,
terminal demos, and following `/var/log/syslog`. Copy an example before customizing
it to preserve the original. JSON examples require `/usr/bin/jq`; log following
requires read access to the log file.

Button and terminal colors use `fg_color` and `bg_color`, with RGB arrays such as
`[35, 105, 75]`. Omit terminal color fields (or use `null`) to follow the theme.
Existing layouts with explicit terminal colors keep them until cleared using
**Appearance → Use theme colors**. Standard ANSI colors adapt to the terminal
background; extended colors (indices 16–255) and explicit RGB output stay exact.

### Terminal output logs

Set `BUTTON_BUILDER_LOG_DIR` in your environment or in `.env` at the project root
(see [.env.example](.env.example)):

```dotenv
BUTTON_BUILDER_LOG_DIR=./logs
```

Enable **Log Output** in a button's edit dialog, or set `log_output: true` in that
button's YAML definition. It is unchecked by default, including when loading
older layouts without this field. Changes apply to newly launched processes.

Enable **Auto Start** in a button's edit dialog, or set `auto_start: true` in its
YAML definition, to open that button in a terminal tab whenever the layout file
is loaded. Each enabled, visible button opens in its own tab. Auto Start is
unchecked by default and does not run when merely saving a layout.

Enable **Close on success** in a button's edit dialog, or set `close_on_success: true`
in its YAML definition, to close its terminal after exit status 0 regardless of
output. It is unchecked by default; unchecked buttons still close on silent success.
Changes apply to new runs, including Auto Start. Closing waits for pending output
and log writes to finish; enabled output logs are preserved. Without logging,
closing discards the displayed output.

Each opted-in process that produces output gets its own log, for example
`logs/run_top/20261117_183100.log`. Button labels become lowercase directory names
with underscores replacing spaces and punctuation. Directories are created as
needed; runs started in the same second get a numeric filename suffix to avoid
overwriting earlier logs. Relative paths resolve from the app's working directory.
Log files and directories are created on the first meaningful text: a parsed,
non-whitespace character with a display width. Whitespace, zero-width Unicode
characters alone, and terminal controls (colors, cursor movement, title changes,
etc.) never create a log, regardless of the exit status. The terminal parser
handles sequences split across reads. Text counts even if later erased or reset;
malformed UTF-8 counts as visible replacement text. Successful tabs close only
after all output and log writes are drained. Up to 64 KiB of preceding terminal
controls are retained to preserve startup styling.

Logs capture the original terminal output stream, including stdout, stderr, ANSI
colors, and screen updates, independently of the 2,000-row scrollback limit.
Hover over the terminal status to see its log path. Write errors appear in that
status. Logging requires both the button's opt-in and a nonempty directory setting.

Writes run off the UI thread with bounded queues. If storage falls behind, process
output pauses until writes catch up; Stop and Close tab remain available. Stopping
or closing a tab finishes logging output already read from the process, while
discarding unread output as usual. Exit allows queued writes to finish, with a
two-second timeout if storage stalls. Log files and `.env` are excluded from Git.

## Development

| Source | Responsibility |
| --- | --- |
| `src/main.rs`, `src/cli.rs` | Startup, arguments, logging, and `.env` loading. |
| `src/app.rs` | Application state, layout persistence, terminal lifecycle, and frame composition. |
| `src/ui/` | Header, button grid/editor, layout dialogs, appearance, terminal sections, and shared styling. |
| `src/app/tests.rs` | UI interaction and application workflow tests. |
| `src/layout_config.rs`, `src/config_picker.rs` | YAML persistence and layout selection. |
| `src/settings.rs` | App preferences and startup layout restoration. |
| `src/terminal/` | Pseudo-terminal processes, input, and screen rendering. |
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
Keep changes focused and include updated GUI screenshots when changing the interface.

Logging uses `RUST_LOG` (default: `info`). Startup loads an optional `.env` file;
child processes inherit the app's environment, including those values.

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
