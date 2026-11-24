//! Terminal process, output, and logging tests.

use super::*;
use std::time::{Duration, Instant};

fn wait_for(terminal: &mut Terminal, predicate: impl Fn(&Terminal) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        terminal.poll();
        if predicate(terminal) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out: {}", terminal.status);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn state(pid: u32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.chars().next()
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(Instant::now() < deadline, "Process cleanup timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn log_directory(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "button-builder-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ))
}

#[test]
fn close_on_success_waits_for_output_and_preserves_logs() {
    for close_on_success in [false, true] {
        for exit_code in [0, 1] {
            for log_output in [false, true] {
                let directory = log_directory("close-on-success");
                let button = ButtonConfig {
                    command: format!(
                        "printf 'useful output'; printf 'stderr output' >&2; exit {exit_code}"
                    ),
                    shell: true,
                    log_output,
                    close_on_success,
                    ..Default::default()
                };
                let mut terminal =
                    Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
                // Observe exit before polling: unread output must prevent closing even with opt-in.
                wait_until(|| {
                    terminal.check_exit();
                    !terminal.is_running()
                });
                assert!(!terminal.should_auto_close());
                wait_for(&mut terminal, |t| !t.needs_poll());
                assert_eq!(terminal.screen.text().trim(), "useful outputstderr output");
                assert_eq!(
                    terminal.should_auto_close(),
                    close_on_success && exit_code == 0
                );
                if log_output {
                    let path = terminal.log_path().unwrap().to_owned();
                    drop(terminal);
                    assert_eq!(std::fs::read(path).unwrap(), b"useful outputstderr output");
                } else {
                    assert!(!directory.exists());
                }
            }
        }
    }
}

#[test]
fn close_on_success_keeps_logging_errors_visible() {
    let directory = log_directory("close-log-error");
    std::fs::write(&directory, "not a directory").unwrap();
    let button = ButtonConfig {
        command: "/usr/bin/printf".into(),
        arguments: "result".into(),
        log_output: true,
        close_on_success: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.exit_status.unwrap().success());
    assert!(terminal.log_error().is_some());
    assert!(!terminal.should_auto_close());
    assert!(terminal.screen.text().contains("result"));
}

#[test]
fn silent_successes_and_failures_never_create_logs() {
    for (command, success) in [("/usr/bin/true", true), ("/usr/bin/false", false)] {
        let directory = log_directory("silent-run");
        let button = ButtonConfig {
            command: command.into(),
            log_output: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        assert!(terminal.log_path().is_none());
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert_eq!(terminal.should_auto_close(), success);
        assert!(terminal.log_error().is_none());
        assert!(terminal.log_path().is_none());
        assert!(
            !directory.exists(),
            "Silent runs must never create log directories"
        );
    }
}

#[test]
fn control_only_output_never_creates_logs_and_only_success_is_silent() {
    for output in [
        "\\r\\033[2K\\033[3D", // Pause EO's exact log bytes.
        " \\t\\r\\n\\000\\007",
        "\\033]0;title\\007\\033Pmetadata\\033\\\\\\033[32m\\033[0m",
        "\u{200b}\u{200d}\u{feff}",
    ] {
        for status in [0, 1] {
            let directory = log_directory("control-only");
            let button = ButtonConfig {
                command: format!("printf '{output}'; exit {status}"),
                shell: true,
                log_output: true,
                ..Default::default()
            };
            let mut terminal =
                Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
            wait_for(&mut terminal, |t| !t.needs_poll());
            assert_eq!(terminal.should_auto_close(), status == 0, "{output:?}");
            assert!(!terminal.screen.has_printed_text());
            assert!(terminal.log_error().is_none());
            assert!(terminal.log_path().is_none());
            assert!(!directory.exists());
        }
    }
}

#[test]
fn incomplete_utf8_at_eof_is_visible_and_logged() {
    let directory = log_directory("incomplete-utf8");
    let button = ButtonConfig {
        command: "printf '\\342\\202'".into(),
        shell: true,
        log_output: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(!terminal.should_auto_close());
    assert!(terminal.screen.text().contains('\u{fffd}'));
    assert_eq!(
        std::fs::read(terminal.log_path().unwrap()).unwrap(),
        "\u{fffd}".as_bytes()
    );
}

#[test]
fn logging_waits_for_first_output_and_preserves_first_and_last_chunks() {
    let directory = log_directory("lazy-log");
    let button = ButtonConfig {
        command: "stty -echo; printf '\\r\\033[2K\\033[3D'; sleep 0.1; printf 'first chunk'; read line; printf 'last chunk'"
            .into(),
        shell: true,
        log_output: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
    assert!(terminal.log_path().is_none());
    assert!(!directory.exists());
    wait_for(&mut terminal, |t| {
        t.log_path()
            .and_then(|path| std::fs::read(path).ok())
            .is_some_and(|bytes| bytes == b"first chunk")
    });
    assert!(terminal.is_running());
    terminal.send(b"\n");
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.exit_status.unwrap().success());
    assert!(!terminal.should_auto_close());
    assert_eq!(
        std::fs::read(terminal.log_path().unwrap()).unwrap(),
        b"first chunklast chunk"
    );
}

#[test]
fn final_output_after_exit_and_a_cleared_screen_are_never_silent() {
    let directory = log_directory("erased-output");
    let button = ButtonConfig {
        command: "printf 'final output\\033[2J\\033[H'".into(),
        shell: true,
        log_output: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
    let pid = terminal.child.as_ref().unwrap().id();
    wait_until(|| state(pid) == Some('Z'));
    terminal.check_exit();
    assert!(terminal.exit_status.unwrap().success());
    assert!(
        !terminal.screen.has_printed_text(),
        "The terminal has not read the queued output yet"
    );
    assert!(
        !terminal.should_auto_close(),
        "Exit status alone cannot close a tab"
    );
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.screen.text().trim().is_empty());
    assert!(!terminal.should_auto_close());
    assert_eq!(
        std::fs::read(terminal.log_path().unwrap()).unwrap(),
        b"final output"
    );
}

#[test]
fn stopped_and_interrupted_silent_runs_keep_tabs_without_creating_logs() {
    for interrupt in [false, true] {
        let directory = log_directory("silent-interruption");
        let button = ButtonConfig {
            command: "/bin/sleep".into(),
            arguments: "60".into(),
            log_output: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        if interrupt {
            terminal.interrupt();
        } else {
            terminal.stop();
        }
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(!terminal.should_auto_close());
        assert!(terminal.log_path().is_none());
        assert!(!directory.exists());
    }
    // Some programs handle SIGINT and still return zero; retain the requested interruption.
    let button = ButtonConfig {
        command: "trap 'exit 0' INT; kill -STOP $$; while :; do :; done".into(),
        shell: true,
        close_on_success: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    let pid = terminal.child.as_ref().unwrap().id();
    wait_until(|| state(pid) == Some('T'));
    terminal.interrupt();
    signal(pid as i32, libc::SIGCONT).unwrap();
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.exit_status.unwrap().success());
    assert!(!terminal.screen.has_printed_text());
    assert!(!terminal.should_auto_close());
}

#[test]
fn logging_requires_both_button_opt_in_and_a_directory() {
    let directory = log_directory("opt-in");
    let mut button = ButtonConfig {
        command: "/usr/bin/printf".into(),
        arguments: "hello".into(),
        ..Default::default()
    };
    let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.screen.text().contains("hello"));
    assert!(terminal.log_path().is_none());
    assert!(
        !directory.exists(),
        "Unchecked buttons must not create log directories"
    );
    button.log_output = true;
    let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, None).unwrap();
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.screen.text().contains("hello"));
    assert!(terminal.log_path().is_none());
    assert!(!directory.exists());
}

#[test]
fn logs_preserve_plain_text_after_scrollback_eviction_and_screen_redraws() {
    let directory = log_directory("full-output");
    let button = ButtonConfig {
        label: "JSON / Events Stream".into(),
        log_output: true,
        command: "stty -onlcr; i=1; while [ \"$i\" -le 2300 ]; do printf 'line:%04d\\n' \"$i\"; i=$((i + 1)); done; printf '\\033[?1049h\\033[31mtransient λ\\033[0m\\rrewritten\\n\\033[?1049l'; printf 'stderr output\\n' >&2".into(),
        shell: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.log_error().is_none());
    assert_eq!(terminal.screen.history.len(), screen::HISTORY_LIMIT);
    assert!(!terminal.screen.text().contains("line:0001"));
    let path = terminal.log_path().unwrap();
    assert_eq!(path.parent().unwrap(), directory.join("json_events_stream"));
    let saved = std::fs::read_to_string(path).unwrap();
    assert_eq!(
        saved
            .lines()
            .filter(|line| line.starts_with("line:"))
            .count(),
        2300
    );
    assert!(saved.starts_with("line:0001\n"));
    assert!(saved.contains("line:2300\n"));
    assert!(saved.contains("transient λ\nrewritten\n"));
    assert!(!saved.contains('\x1b'));
    assert!(saved.ends_with("stderr output\n"));
}

#[test]
fn concurrent_runs_log_live_output_separately_and_finish_after_stop_or_close() {
    let directory = log_directory("concurrent-logs");
    let mut terminals = Vec::new();
    for name in ["alpha", "beta"] {
        let button = ButtonConfig {
            label: "Run Top".into(),
            log_output: true,
            command: format!("stty -onlcr; printf '{name}-ready\\n'; read line"),
            shell: true,
            ..Default::default()
        };
        terminals.push(Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap());
    }
    for (terminal, name) in terminals.iter_mut().zip(["alpha", "beta"]) {
        wait_for(terminal, |t| {
            t.screen.text().contains("ready")
                && t.log_path()
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .is_some_and(|text| text == format!("{name}-ready\n"))
        });
        assert!(
            terminal.is_running(),
            "Logs must be visible while the process runs"
        );
    }
    let paths: Vec<_> = terminals
        .iter()
        .map(|t| t.log_path().unwrap().to_owned())
        .collect();
    assert_ne!(paths[0], paths[1]);
    terminals[0].stop();
    drop(terminals);
    finish_cleanup();
    assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "alpha-ready\n");
    assert_eq!(std::fs::read_to_string(&paths[1]).unwrap(), "beta-ready\n");
}

#[test]
fn log_creation_errors_are_reported_without_blocking_process_controls() {
    let directory = log_directory("log-failure");
    std::fs::write(&directory, "not a directory").unwrap();
    let button = ButtonConfig {
        label: "Invalid log directory".into(),
        log_output: true,
        command: "printf ready; read line".into(),
        shell: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
    wait_for(&mut terminal, |t| {
        t.log_error().is_some() && t.screen.text().contains("ready")
    });
    let started = Instant::now();
    terminal.stop();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!terminal.needs_poll());
    assert!(terminal.log_error().is_some());
}

#[test]
fn stopped_non_reading_child_cannot_block_poll_interrupt_or_stop() {
    let button = ButtonConfig {
        command: "trap '' INT TERM; stty -echo -icanon; printf 'ready\\n'; kill -STOP $$; sleep 60"
            .into(),
        shell: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    let pid = terminal.child.as_ref().unwrap().id();
    wait_for(&mut terminal, |t| {
        t.screen.text().contains("ready") && state(pid) == Some('T')
    });
    terminal.send(&vec![b'x'; 65_536]);
    let started = Instant::now();
    for _ in 0..20 {
        terminal.poll();
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!terminal.input.is_empty());
    terminal.interrupt();
    assert!(
        terminal.input.is_empty(),
        "Interrupt must bypass queued input"
    );
    terminal.poll();
    assert!(
        terminal.is_running(),
        "This child intentionally ignores SIGINT"
    );
    let started = Instant::now();
    terminal.stop();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!terminal.needs_poll());
    assert!(terminal.master.is_none());
    assert_eq!(terminal.status, "Stopped");
    wait_until(|| state(pid).is_none());
}

#[test]
fn interrupt_reaches_the_foreground_process_in_raw_mode() {
    let button = ButtonConfig {
        command: "stty raw -echo; printf ready; exec sleep 60".into(),
        shell: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    wait_for(&mut terminal, |t| t.screen.text().contains("ready"));
    terminal.interrupt();
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.status.contains("signal: 2"), "{}", terminal.status);
}

#[test]
fn closing_and_shell_exit_kill_background_jobs_in_separate_process_groups() {
    for wait in [true, false] {
        let button = ButtonConfig {
            command: format!(
                "set -m; trap '' HUP; sleep 60 & printf 'job:%s\\n' \"$!\"; {}",
                if wait { "wait" } else { "exit 0" }
            ),
            shell: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        let leader = terminal.child.as_ref().unwrap().id();
        wait_for(&mut terminal, |t| t.screen.text().contains("job:"));
        let job: u32 = terminal
            .screen
            .text()
            .lines()
            .find_map(|line| line.strip_prefix("job:"))
            .unwrap()
            .parse()
            .unwrap();
        if wait {
            // SAFETY: querying the group of our test's background job does not send a signal.
            assert_ne!(unsafe { libc::getpgid(job as i32) }, leader as i32);
            assert!(terminal.is_running());
            let started = Instant::now();
            drop(terminal);
            assert!(started.elapsed() < Duration::from_millis(500));
            finish_cleanup();
        } else {
            wait_for(&mut terminal, |t| !t.needs_poll());
            assert!(terminal.status.contains("exit status: 0"));
        }
        wait_until(|| matches!(state(job), None | Some('Z')));
        wait_until(|| state(leader).is_none());
    }
}

#[test]
fn child_has_a_tty_receives_input_and_exits() {
    let button = ButtonConfig {
        command: "/bin/sh".into(),
        arguments: "-c 'test -t 0 && test -t 1 && test -t 2 && stty size; read value; printf \"received:%s\\n\" \"$value\"'".into(),
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    assert!(terminal.needs_poll());
    wait_for(&mut terminal, |t| t.screen.text().contains("24 80"));
    terminal.send(b"hello\r");
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(!terminal.is_running());
    assert!(terminal.screen.text().contains("received:hello"));
}

#[test]
fn terminfo_reports_colors_and_styles_output() {
    let button = ButtonConfig {
        command: "printf 'colors:'; /usr/bin/tput colors; /usr/bin/tput setaf 2; printf 'green'; /usr/bin/tput sgr0".into(),
        shell: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.status.contains("exit status: 0"));
    assert_eq!(terminal.screen.text().trim(), "colors:256\ngreen");
    assert_eq!(
        terminal.screen.line(1)[0].style.fg_color,
        Some(colors::Color::Indexed(2))
    );
}

#[test]
fn interactive_htop_renders_colors_and_accepts_quit() {
    if !Path::new("/usr/bin/htop").is_file() {
        eprintln!("Skipping htop integration check: /usr/bin/htop is unavailable");
        return;
    }
    // Use a fresh config so a user's monochrome theme cannot affect the check.
    let config = format!("HTOPRC={}", log_directory("htop-config").display());
    let button = ButtonConfig {
        command: "/usr/bin/env".into(),
        arguments: format!(
            "{} /usr/bin/htop --no-mouse",
            shlex::try_quote(&config).unwrap()
        ),
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 100, 30).unwrap();
    wait_for(&mut terminal, |t| t.screen.text().contains("PID"));
    let colored = (0..30).any(|row| {
        terminal.screen.line(row).iter().any(|cell| {
            cell.style.fg_color.is_some_and(|color| {
                let [r, g, b] = colors::Palette::new(true, None, None).resolve(color);
                r != g || g != b
            })
        })
    });
    terminal.resize(90, 25);
    terminal.send(b"q");
    wait_for(&mut terminal, |t| !t.needs_poll());
    assert!(terminal.status.contains("exit status: 0"));
    assert!(
        colored,
        "htop should detect and emit colored terminal output"
    );
}

#[test]
fn interactive_top_renders_and_accepts_quit() {
    let button = ButtonConfig {
        command: "/usr/bin/top".into(),
        arguments: "-d 0.1".into(),
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 100, 30).unwrap();
    wait_for(&mut terminal, |t| t.screen.text().contains("PID"));
    terminal.resize(90, 25);
    terminal.send(b"q");
    wait_for(&mut terminal, |t| !t.is_running());
    assert!(
        terminal.status.contains("exit status: 0"),
        "{}",
        terminal.status
    );
}

#[test]
fn shell_pipeline_streams_json_through_jq() {
    let button = ButtonConfig {
        command: r#"printf '%s\n' '{"message":"hello"}' '{"message":"world"}' | /usr/bin/jq --unbuffered -r .message"#.into(),
        shell: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    wait_for(&mut terminal, |t| !t.is_running());
    terminal.poll();
    assert!(
        terminal.screen.text().contains("hello\nworld"),
        "{}",
        terminal.screen.text()
    );
    assert!(terminal.status.contains("exit status: 0"));
}

#[test]
fn throttled_output_is_preserved_until_the_terminal_is_fully_drained() {
    let button = ButtonConfig {
        command: "i=1; while [ \"$i\" -le 1000 ]; do printf 'line:%04d:abcdefghijklmnopqrstuvwxyz0123456789\\n' \"$i\"; i=$((i + 1)); done".into(),
        shell: true,
        ..Default::default()
    };
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    wait_for(&mut terminal, |t| !t.needs_poll());
    let output = terminal.screen.text();
    assert_eq!(
        output
            .lines()
            .filter(|line| line.starts_with("line:"))
            .count(),
        1000
    );
    assert!(output.contains("line:0001:abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(output.contains("line:1000:abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(terminal.status.contains("exit status: 0"));
}

#[test]
fn launch_errors_are_reported_and_stop_reaps_child() {
    let mut button = ButtonConfig {
        command: "/no/such/program".into(),
        ..Default::default()
    };
    assert!(Terminal::start(&button, 80, 24).is_err());
    button.command = "/bin/sleep".into();
    button.arguments = "60".into();
    let mut terminal = Terminal::start(&button, 80, 24).unwrap();
    let pid = terminal.child.as_ref().unwrap().id();
    terminal.stop();
    assert!(!terminal.is_running());
    assert_eq!(terminal.status, "Stopped");
    wait_until(|| state(pid).is_none());
}

#[test]
fn every_bundled_example_command_runs_locally() {
    for name in [
        "system-monitor.yaml",
        "json-streams.yaml",
        "terminal-demo.yaml",
        "log-tail.yaml",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(name);
        let config = crate::layout_config::Config::load_existing(&path).unwrap();
        assert_eq!(config.buttons.len(), config.visible_buttons());
        for button in &config.buttons {
            let mut button = button.clone();
            let log = if button.command == "/usr/bin/tail" {
                assert_eq!(
                    button.args().unwrap(),
                    ["-n", "50", "-F", "/var/log/syslog"]
                );
                let directory = std::env::temp_dir().join(format!(
                    "rs-button-builder-tail-test-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                std::fs::create_dir(&directory).unwrap();
                let path = directory.join("syslog");
                std::fs::write(&path, "existing log entry\n").unwrap();
                button.arguments = format!(
                    "-n 50 -F --sleep-interval=0.1 {}",
                    shlex::try_quote(path.to_str().unwrap()).unwrap()
                );
                Some(path)
            } else {
                None
            };
            let mut terminal = Terminal::start(&button, 120, 32)
                .unwrap_or_else(|error| panic!("{name} / {}: {error}", button.label));
            if button.command == "/usr/bin/top" {
                wait_for(&mut terminal, |t| t.screen.text().contains("PID"));
                terminal.send(b"q");
            } else if button.label == "Input Echo" {
                wait_for(&mut terminal, |t| t.screen.text().contains("Your name:"));
                terminal.send(b"Example\r");
            } else if let Some(path) = log {
                wait_for(&mut terminal, |t| {
                    t.screen.text().contains("existing log entry")
                });
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap();
                writeln!(file, "appended log entry").unwrap();
                wait_for(&mut terminal, |t| {
                    t.screen.text().contains("appended log entry")
                });
                std::fs::rename(&path, path.with_extension("1")).unwrap();
                std::fs::write(&path, "rotated log entry\n").unwrap();
                wait_for(&mut terminal, |t| {
                    t.screen.text().contains("rotated log entry")
                });
                assert!(
                    terminal.is_running(),
                    "Following must continue after rotation"
                );
                terminal.send(b"\x03");
                wait_for(&mut terminal, |t| !t.is_running());
                assert!(terminal.status.contains("signal: 2"), "{}", terminal.status);
                continue;
            }
            wait_for(&mut terminal, |t| !t.is_running());
            terminal.poll();
            assert!(
                terminal.status.contains("exit status: 0"),
                "{name} / {}: {}",
                button.label,
                terminal.status
            );
            assert!(
                !terminal.screen.text().trim().is_empty(),
                "{name} / {} produced no output",
                button.label
            );
        }
    }
}
