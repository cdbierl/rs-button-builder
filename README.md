# Button Builder

A Linux egui app with a configurable command grid and an embedded terminal.
The interface uses `egui-material3` controls with an orange accent, warm dark
surfaces, and small corners on buttons, dialogs, inputs, and menus. Dialog frames
use the Material theme with a shared 6-pixel radius. Text and numeric inputs use a matching theme.
Fonts and theme colors are loaded locally; optional downloadable assets are disabled.

```sh
cargo run --offline
cargo run --offline -- my-buttons.yaml
cargo run --offline -- --help
```

The first button is **Run Top**, using `/usr/bin/top`. Click an unassigned button,
right-click a button, or enable **Edit buttons** to configure its label and action.
Uncheck **Edit buttons** to launch commands. Saving a button does not run it.
The editor also has **Text color** and **Background** pickers, a live button preview,
and **Reset colors** to restore the theme defaults. Colors are saved per button.
When only a background is set, the label automatically uses contrasting black or white text.

Use **Rows** and **Columns** above the grid to change its dimensions (1–12 rows,
1–8 columns; initially 3×3). The grid scrolls when it exceeds the available space.
Buttons keep their order from left to right, then top to bottom. Shrinking hides
extra buttons without deleting their labels or commands; expanding restores them.
Use **Save layout** in the header toolbar to persist the dimensions along with the
button definitions. An asterisk marks unsaved layout or appearance changes.

Click **Load config…** in the header toolbar to browse local YAML files or enter a
path. The chooser's **Examples** button opens the bundled [example layouts](examples/README.md): system
monitoring, local JSON pipelines, terminal demonstrations, and a live log tail. Loading restores the
grid, button definitions/colors, and terminal appearance, and selects that file as
the save destination. A running process continues in the terminal. Unsaved changes
must be saved or explicitly replaced; missing or invalid files leave the current
layout intact.

For a program, enter the executable separately from its optional arguments. Quote
arguments containing spaces. A blank working directory inherits the app's working
directory. Programs must already be available locally.

For a pipeline, select **Shell pipeline** and enter the entire command. For example,
if `WS_URL` is already set to your WebSocket endpoint and the messages are JSON:

```sh
wscat --no-color -c "$WS_URL" | jq --unbuffered .
```

Shell mode runs `/bin/sh -c`, enabling pipes, redirection, and environment-variable
expansion. Program mode passes the parsed arguments directly to the executable.
The existing `.env` loading in `main` also applies to launched commands.

The terminal uses a real pseudo-terminal, so interactive commands such as `top`
receive a terminal size and keyboard input. Click the pane to type; `q` quits `top`.
**Ctrl+C** sends an interrupt, and **Stop** forcibly ends the running process group.
Only one command runs at a time. Closing the window stops the active process.
Output remains visible after a command exits; starting another command replaces it.
Keep shell commands in the foreground: background jobs that outlive their shell
are no longer tracked by **Stop** or window cleanup.

Use **Appearance** to change the monospace font, font size, line spacing, and text
and background colors. Built-in monospace is always available. Installed monospace
fonts are discovered through Fontconfig and user font directories, including
`~/.local/share/fonts`, `~/.fonts`, and `$XDG_DATA_HOME/fonts`. Nested directories
are scanned. Click **Refresh fonts** after installing more fonts. Scroll
over the terminal for up to 2,000 lines of history, and use **Jump to live** to
return to current output. **Copy output** or Ctrl+Shift+C copies the retained text.

**Save button**, **Save appearance**, and **Save layout** write buttons and appearance
to the selected YAML file (default: `./buttons.yaml`). Appearance changes preview
immediately and need saving to persist. If the default `buttons.yaml` is missing,
the app starts with the default layout. An explicitly selected missing file or a
malformed file reports an error rather than being overwritten.

The embedded screen supports VT100 cursor movement, erase operations, scrolling
regions, ANSI colors, Unicode, and common alternate-screen sequences. It is a small
terminal implementation: terminal graphics, mouse reporting, and full text reflow
on width changes are not implemented. Commands receive `TERM=vt100`. Font/window
changes notify the process of its new terminal dimensions.

Checks use local commands and generated data, including interactive `top` and a
JSON pipeline through `/usr/bin/jq`; no WebSocket connection is opened by the tests.

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
```
