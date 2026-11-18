# Example layouts

Click **Load config…**, then **Examples**, select a YAML file, and click
**Load selected config**. You can also start with one directly:

```sh
cargo run --locked -- examples/system-monitor.yaml
```

| File | Grid | Buttons |
| --- | --- | --- |
| `system-monitor.yaml` | 3×3 | Interactive `top`, memory, disk space, processes, and other local Linux reports. |
| `json-streams.yaml` | 2×3 | Generated JSON, streaming events, filtering, field selection, summaries, and text conversion through `jq`. |
| `terminal-demo.yaml` | 2×2 | ANSI colors, stdout/stderr, progress redraws, Unicode, and interactive input. |
| `log-tail.yaml` | 1×1 | Shows the last 50 lines of `/var/log/syslog`, then follows new entries and log rotation. |

These examples use local commands, system logs, and generated data. They do not open network
connections. The JSON layout requires `/usr/bin/jq`; the system layout uses common
Linux tools under `/usr/bin`. Loading a file displays its buttons; commands start
when clicked.

Quit **Run Top** with `q`. For **Input Echo**, click the terminal, type a name, and
press Enter. **Stream Events** and **Progress Demo** finish automatically.
**Follow Syslog** runs `/usr/bin/tail -n 50 -F /var/log/syslog` until Ctrl+C or
**Stop**. The current user needs read access to `/var/log/syslog`.
`-F` follows the filename and retries,
so it also handles files being rotated and recreated.

To follow one of your own logs, configure a program button with `/usr/bin/tail`
and arguments such as `-n 50 -F "/path/to/your.log"`.

The loaded file becomes the destination for subsequent saves. Copy an example to
your own YAML file to keep a separate customized layout. Colors are RGB arrays
such as `[35, 105, 75]`; omitted fields use the app's defaults.
Each example includes a `display_name`, shown above the button grid. Click that
heading to rename the loaded layout, then use **Save layout** to persist it.
