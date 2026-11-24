# AGENTS.md

This file applies to the entire repository.

## Project

Button Builder is a Linux desktop application that turns reusable YAML command
layouts into a button grid with interactive terminal tabs and split panes. It is
an executable application built with Rust 2024, egui, and egui-material3.

Keep changes focused on the requested behavior. Prefer the smallest design that
preserves the existing interaction model and module boundaries.

## Build and validation

Use the locked dependency set:

```sh
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Run focused tests while iterating, then run the full test suite for changes that
affect shared UI state, terminal behavior, persistence, processes, or layouts.
The full suite is configured to run serially because UI tests share global theme
state and process tests contain timing assertions.

Do not add tests for trivial styling changes. Add or update tests when behavior,
state transitions, parsing, persistence, process handling, or terminal rendering
changes.

Some terminal integration tests use local Linux executables such as `top`, `jq`,
and `tput`; `htop` coverage is conditional on it being installed.

## Source organization

- `src/app.rs`: top-level application state, persistence orchestration, notices,
  and frame composition.
- `src/workspace/`: terminal tabs, pane membership, selection, process polling,
  repaint scheduling, and nested pane state.
- `src/ui/`: one module per logical UI section, plus shared Material styling.
- `src/terminal/`: pseudo-terminal lifecycle, bounded output processing, screen
  emulation, process cleanup, and plain-text logging.
- `src/layout_config.rs`: YAML layout schema, defaults, validation, loading, and
  saving.
- `src/settings.rs`: application preferences under
  `$XDG_CONFIG_HOME/rs_button_builder`.
- `src/theme.rs`: application theme and terminal palette defaults.
- `src/app/tests/`: end-to-end application and UI-state tests.

Place state with its owner:

- Persistent button and terminal defaults belong in the layout configuration.
- Application preferences belong in settings.
- Runtime terminal, tab, and pane state belongs in the workspace.
- Temporary widget interaction state belongs in the relevant UI component.

Do not grow `app.rs` with section-specific rendering. Extend an existing UI
module or add a focused module under `src/ui/`.

## Rust conventions

- Follow rustfmt and keep Clippy clean with warnings denied.
- Use narrow visibility such as `pub(super)` or `pub(in crate::app)` unless a
  broader API is required.
- Add a short module-level `//!` comment that states the module's responsibility.
- Prefer constants for shared dimensions, limits, timing, and layout constraints.
- Avoid duplicating state or deriving the same value in multiple modules.
- Preserve useful errors and log failures rather than silently discarding them.
- Use `log::info!` for process lifecycle and logging events; use `log::debug!`
  for tab, pane, display, and interaction details.

## UI conventions

- Use Material semantic colors through `get_global_color`; do not hard-code theme
  colors when a semantic token exists.
- Reuse controls and helpers from `src/ui/style.rs`, `button.rs`, and
  `checkbox.rs`.
- Use sentence case for labels and concise action-oriented tooltips.
- Use Material Symbols from the bundled local font for icon actions.
- Selected and toggled controls should use the active accent theme.
- Keep layouts usable in narrow split panes. Prefer hiding lower-priority toolbar
  actions over wrapping toolbars into extra rows.
- Preserve balanced pane padding and ensure strokes are painted inside clip
  boundaries.
- Keep light, dark, and custom terminal colors working for already-running tabs.
- Do not add screenshots to the README.

## Terminal and process invariants

- Never block the egui frame on process I/O, logging, cleanup, or long parsing.
- Keep output queues and scrollback bounded.
- Preserve PTY sizing, interactive input, ANSI colors, Unicode cell widths,
  alternate-screen behavior, and process-group cleanup.
- Resolve terminal theme colors while painting so existing output follows theme
  changes unless the tab has explicit colors.
- Log files must contain plain UTF-8 text without ANSI escape sequences or other
  terminal controls.
- Pausing display updates must not pause the process or background logging.
- Closing, stopping, and interrupting have different semantics; do not combine
  them without reviewing the existing process tests.
- Silent successful commands close automatically. `close_on_success` extends
  that behavior to any status-0 exit. Failures and terminal or logging errors
  must remain visible.

## Configuration and repository hygiene

- Keep `Cargo.lock` tracked and use `--locked` for validation.
- Before adding a dependency, confirm the standard library or an existing direct
  dependency cannot reasonably provide the behavior.
- Preserve backward-compatible YAML defaults where practical.
- Settings and layouts are separate formats; do not put runtime-only UI state in
  either one.
- Do not modify user-created YAML layouts, `.env`, logs, or files outside the
  repository unless the task explicitly requires it.
- Do not commit build output, logs, local settings, or temporary profiling data.
- Update README text when user-visible behavior, configuration, shortcuts,
  requirements, or architecture changes.
