# Developing Button Builder

This guide explains the architecture and maintenance workflows for Button
Builder. See [AGENTS.md](AGENTS.md) for repository rules and [README.md](README.md)
for user-facing behavior.

## Development setup

Button Builder targets Linux and uses Unix pseudo-terminals and Linux process
APIs. Development requires Rust 1.88 or newer, Cargo, a C toolchain/linker, and
an X11 or Wayland desktop with OpenGL support.

Build and run with the locked dependency set:

```sh
cargo build --locked
cargo run --locked
cargo run --locked -- --file examples/system-monitor.yaml
```

Startup loads `.env` when present. Application logs go to stderr:

```sh
RUST_LOG=debug cargo run --locked
```

`info` includes process lifecycle and output-log events. `debug` also includes
tab selection, pane changes, display pauses, and other UI state transitions.

## Architecture

The application has four main layers:

```text
main / CLI
    |
    v
ButtonBuilder (application and persistent layout state)
    |
    +-- UI modules (rendering and temporary interaction state)
    |
    +-- TerminalWorkspace (tabs, panes, focus, polling)
            |
            +-- Terminal (PTY, child process, screen, logging)
```

### Startup and frame flow

`src/main.rs` loads `.env`, initializes logging, parses command-line arguments,
loads application settings, resolves the startup layout, and starts eframe.

Each application frame in `ButtonBuilder::show`:

1. Synchronizes the selected theme.
2. Polls terminal sessions within a shared time budget.
3. Handles application shortcuts.
4. Renders the footer, optional button panel, and terminal workspace.
5. Renders dialogs above the main interface.
6. Updates the window title when the layout name or dirty state changes.
7. Schedules another repaint only when terminal work or a timed notice requires it.

Avoid unconditional repaints. Continuous terminal output should wake the UI or
schedule a short delayed repaint; idle sessions should not keep the frame loop busy.

### State ownership

`ButtonBuilder` owns the loaded layout, settings store, theme, dialogs, editor
drafts, dirty state, and the terminal workspace.

`TerminalWorkspace` owns runtime terminal state:

- Terminal sessions and stable tab IDs
- Tab-to-button and tab-to-pane associations
- The nested split-pane tree and divider fractions
- Active tab and focused pane
- Deferred structural pane actions
- Polling fairness and repaint scheduling

UI modules own section-specific rendering and short-lived interaction state.
Runtime-only details such as scrollback position, selection, find state, paused
snapshots, splits, and running processes are not persisted.

## Source map

| Source | Responsibility |
| --- | --- |
| `src/main.rs`, `src/cli.rs` | Startup, environment, CLI parsing, and native window creation |
| `src/app.rs` | Top-level state, persistence orchestration, dialogs, and frame composition |
| `src/workspace/mod.rs` | Terminal tabs, focus, pane membership, polling, and process actions |
| `src/workspace/panes.rs` | Nested pane tree, divider fractions, and empty-pane collapse |
| `src/ui/` | Logical UI sections, terminal rendering, input, and shared Material controls |
| `src/terminal/mod.rs` | PTY lifecycle, process groups, nonblocking I/O, and cleanup |
| `src/terminal/screen.rs` | Bounded terminal screen, ANSI parsing, Unicode cells, and history |
| `src/terminal/logging/` | Bounded asynchronous conversion to plain-text log files |
| `src/layout_config.rs` | User layout schema, defaults, validation, and atomic YAML writes |
| `src/settings.rs` | Theme, last layout, and recent-layout application preferences |
| `src/theme.rs` | Theme mode, accent choice, and terminal palette defaults |
| `src/app/tests/` | Application and UI interaction tests |
| `src/terminal/tests.rs` | PTY, process, output, and logging integration tests |

`src/ui/mod.rs` is the UI module boundary. Keep section-specific rendering out
of `app.rs`; add behavior to the closest existing UI module or create a focused
module with a module-level responsibility comment.

## Terminal lifecycle

Each button launch creates a pseudo-terminal master/slave pair. The child starts
in a new session with the slave as its controlling terminal and standard streams.
Direct commands execute without an implicit shell. Buttons configured for shell
mode execute `/bin/sh -c`.

The child receives `TERM=xterm-256color`. PTY reads and writes are nonblocking.
A watcher wakes egui when I/O becomes ready, while periodic polling handles child
exit and missed wakeups.

The distinction between terminal actions matters:

- **Interrupt** sends the terminal interrupt behavior to the foreground process.
- **Stop** terminates the process group and discards unread output.
- **Close** removes the tab and requests asynchronous process cleanup.
- **Auto close** occurs after all output is drained for a qualifying successful exit.

Dropping a terminal must not wait for process cleanup on the UI thread. Cleanup
and reaping run through the background cleanup worker, and application shutdown
briefly waits for outstanding cleanup.

### Screen and viewport

`Screen` stores terminal cells and parses common VT100/xterm control sequences.
History is limited to 2,000 lines and individual screen lines are bounded. The
screen retains color and style information; terminal theme colors are resolved
when painted so running tabs can follow light/dark theme changes.

`TerminalViewport` maps retained logical lines to the visible pane. Line wrapping
is a view option and does not rewrite the PTY screen. Horizontal scrolling is
available when wrapping is off.

Selections use logical terminal rows and cell columns so they continue to work
across scrollback. Drag selection, double-click token selection, and triple-click
line selection copy plain text automatically.

### Pausing and find

Pausing display updates clones the current screen into a per-tab snapshot. The
child process, PTY polling, and output logging continue in the background.

Terminal find searches that snapshot. It tracks whether find initiated the pause,
so closing find only resumes a display that it paused. Query text, options, matches,
and current result belong to the tab and are never saved to layout or app settings.

### Output logging

Logging runs on a bounded worker channel. If disk writes fall behind, PTY reads
yield until queue capacity returns instead of blocking the UI or growing memory
without limit.

A log file is created only after meaningful printable output appears. Raw PTY
bytes pass through the plain-text converter, which removes ANSI formatting and
terminal controls. Logs contain UTF-8 text and continue while display updates are
paused.

## Pane layout

Pane structure is a binary tree. Each split node stores a direction, stable
divider ID, fraction, and two children. Leaves contain stable pane IDs and the
selected tab ID.

Geometry is calculated every frame from the tree. Each leaf has a preferred
minimum size of 240 by 220 points, dividers are 8 points wide, and split panes use
10 points of perimeter padding. When the window is too small, space is distributed
proportionally without discarding the user's preferred divider fraction.

Split, move, merge, and tab-menu close actions are queued while rendering and
applied afterward. This prevents structural mutations while the UI is iterating
through geometry derived from the previous tree.

Pane layout is intentionally runtime-only. A future persistence feature should
use a separate workspace representation rather than add runtime process state to
the button-layout YAML.

## Configuration and settings

Button layouts and application settings have separate schemas.

Layout YAML stores:

- Display name and grid dimensions
- Button commands and launch options
- Button colors
- Default terminal appearance

Application settings store:

- Theme mode and accent
- Last opened layout
- Up to eight recent layout paths

Settings live under `$XDG_CONFIG_HOME/rs_button_builder/settings.yaml`, falling
back to `~/.config/rs_button_builder/settings.yaml`. Stored layout paths are made
absolute. Both formats use defaults for missing fields, and writes use a temporary
file followed by an atomic rename.

When adding fields:

1. Decide whether the value is layout data, an app preference, or runtime-only.
2. Add a backward-compatible default.
3. Validate and clamp external numeric input.
4. Add round-trip and malformed-input coverage where behavior changes.
5. Update README examples when users need to know the field.

## UI conventions

Use shared controls and styling from `src/ui/style.rs`, `button.rs`, and
`checkbox.rs`. Material semantic colors should come from `get_global_color`;
selected states should follow the active accent.

Terminal panes can become narrow after repeated splits:

- Keep one left control group and one right control group in toolbars.
- Hide lower-priority controls as width decreases instead of adding toolbar rows.
- Paint borders inside clip rectangles.
- Preserve balanced inner padding on both sides.
- Test both light and dark themes when changing selected, hover, or disabled states.

Application shortcuts are centralized in `src/ui/shortcuts.rs`. Widget-local
shortcuts, such as find navigation, should be consumed only while that widget is
active so terminal applications receive the same keys otherwise.

## Performance model

Terminal processing is explicitly bounded:

- A terminal poll receives a 2 ms parsing deadline and a byte limit.
- The workspace shares a 4 ms polling budget across tabs.
- Polling starts at a rotating tab so noisy sessions cannot starve others.
- Active work schedules repaints at roughly 30 frames per second.
- Child-exit recovery schedules a one-second check when necessary.
- Terminal history, line width, input/output buffers, and log queues are bounded.
- Visible terminal text layouts are cached and invalidated when fonts, scale, or
  theme changes make them stale.

When investigating performance:

1. Reproduce with both idle and high-output terminals.
2. Test one pane, multiple visible panes, and hidden tabs separately.
3. Check for unconditional `request_repaint` calls.
4. Confirm work scales with visible rows or bounded queues rather than process age.
5. Preserve fairness between multiple noisy tabs.

## Testing

The Cargo configuration sets `RUST_TEST_THREADS=1`. UI tests share global Material
theme state, and process tests include elapsed-time assertions.

Run a focused test while iterating:

```sh
cargo test --locked terminal_search
cargo test --locked terminal_selection
cargo test --locked terminal_split
```

Before submitting behavior or architectural changes, run:

```sh
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Tests use generated data and local tools including `top`, `jq`, and `tput`.
`htop` tests run only when it is installed. Do not make network access a test
requirement.

Use application tests for behavior spanning state and UI interaction. Use focused
unit tests for parsing, geometry, cell mapping, and other deterministic helpers.
Use terminal integration tests when process groups, PTY behavior, output draining,
logging, or cleanup semantics change.

## Common changes

### Add an application shortcut

1. Add an `Action` in `src/ui/shortcuts.rs`.
2. Define its exact modifiers and key.
3. Add it to the handled action list and action dispatcher.
4. Ensure repeats are consumed without repeating destructive actions.
5. Update the README shortcut table and interaction tests.

### Add a terminal toolbar action

1. Add the control to `src/ui/terminal_toolbar.rs`.
2. Define where it falls in the narrow-pane visibility priority.
3. Select the affected tab before mutating it.
4. Keep process or tab state changes in the workspace or terminal owner.
5. Test minimum-width and split-pane layouts when behavior is width-sensitive.

### Add a pane operation

1. Model structural changes as `PaneAction`.
2. Queue the action during rendering.
3. Apply it after all panes render.
4. Normalize empty panes and selected tab IDs.
5. Test horizontal, vertical, nested, and minimum-window layouts.

### Add terminal escape handling

1. Extend `src/terminal/screen.rs`.
2. Preserve parser progress across fragmented input.
3. Bound any new retained state.
4. Add tests for complete, fragmented, malformed, and reset sequences.
5. Verify resize, history, and alternate-screen behavior where relevant.

### Add a setting or layout property

Choose the owning format first. Add a default and serialization coverage, handle
invalid or missing values, and update user documentation. Do not persist temporary
widget state or live workspace/process state.

## Known constraints

- Linux is the supported platform.
- Terminal graphics and terminal application mouse-reporting protocols are not
  implemented.
- Width changes preserve retained cells but do not fully reflow historical text.
- Pane layout and running sessions are not restored on startup.
- Detached jobs that create a new session can outlive the process group managed
  by the application.
- A process blocked in uninterruptible kernel I/O may not exit promptly after
  `SIGKILL`.

## Review checklist

- The change is in the module that owns the state or behavior.
- The UI thread performs no blocking process, disk, or cleanup work.
- New queues, history, or retained text have explicit bounds.
- Narrow split panes and both theme modes remain usable.
- Errors remain visible and useful.
- Layout YAML remains backward compatible where practical.
- README, examples, and shortcuts reflect user-visible changes.
- Locked tests, Clippy, formatting, and `git diff --check` pass.
