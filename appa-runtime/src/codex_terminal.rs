//! A live APPA footer below the Codex terminal on Unix.
//!
//! Codex exposes built-in footer items only. Native command support is requested
//! in https://github.com/openai/codex/issues/17827 and can replace this wrapper.

use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize};
use unicode_width::UnicodeWidthChar;

use crate::elicit::{MARK_BOTTOM, MARK_TOP};

const STATUS_FILE: &str = "APPA_CODEX_STATUS_FILE";
const FOOTER_ROWS: u16 = 2;

pub(crate) fn is_interactive(arguments: &[OsString]) -> bool {
    io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && io::stderr().is_terminal()
        && interactive_arguments(arguments)
}

fn interactive_arguments(arguments: &[OsString]) -> bool {
    if arguments
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| matches!(arg.to_str(), Some("--help" | "-h" | "--version" | "-V")))
    {
        return false;
    }
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let text = argument.to_string_lossy();
        if text == "--" {
            return true;
        }
        if matches!(text.as_ref(), "--help" | "-h" | "--version" | "-V") {
            return false;
        }
        if matches!(
            text.as_ref(),
            "-c" | "--config"
                | "-C"
                | "--cd"
                | "-p"
                | "--profile"
                | "-m"
                | "--model"
                | "--enable"
                | "--disable"
                | "--add-dir"
                | "-i"
                | "--image"
                | "-a"
                | "--ask-for-approval"
                | "-s"
                | "--sandbox"
                | "--local-provider"
        ) {
            arguments.next();
            continue;
        }
        if !text.starts_with('-') {
            return !matches!(
                text.as_ref(),
                "exec"
                    | "e"
                    | "review"
                    | "login"
                    | "logout"
                    | "mcp"
                    | "mcp-server"
                    | "app-server"
                    | "completion"
                    | "sandbox"
                    | "debug"
                    | "apply"
                    | "a"
                    | "cloud"
                    | "features"
                    | "help"
            );
        }
    }
    true
}

/// Only the accepted root SessionStart hook selects the footer's trajectory.
pub(crate) fn record_session(input: &[u8], accepted: bool) {
    let Some(path) = std::env::var_os(STATUS_FILE) else {
        return;
    };
    let session = serde_json::from_slice::<serde_json::Value>(input)
        .ok()
        .filter(|event| accepted && event["agent_id"].is_null())
        .and_then(|event| event["session_id"].as_str().map(str::to_owned));
    let contents = serde_json::to_vec(&session).expect("a session id serializes");
    // A display failure cannot change the hook decision.
    let _ = fs::write(path, contents);
}

fn status_updates(url: String, path: std::path::PathBuf) -> mpsc::Receiver<Option<String>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        loop {
            let chips = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Option<String>>(&bytes).ok().flatten())
                .and_then(|id| crate::statusline::trajectory_chips(&url, &format!("codex:{id}")));
            if sender.send(chips).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    receiver
}

fn terminal_size() -> io::Result<PtySize> {
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    // The initialized structure remains valid for the complete ioctl call.
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(PtySize {
        rows: size.ws_row.max(1),
        cols: size.ws_col.max(1),
        pixel_width: 0,
        pixel_height: 0,
    })
}

fn child_size(size: PtySize) -> PtySize {
    PtySize {
        rows: size.rows.saturating_sub(FOOTER_ROWS).max(1),
        ..size
    }
}

struct Terminal {
    original: libc::termios,
    signals: Vec<signal_hook::SigId>,
    keyboard_depth: Arc<AtomicUsize>,
}

impl Terminal {
    fn enter(signal: &Arc<AtomicUsize>, keyboard_depth: Arc<AtomicUsize>) -> io::Result<Self> {
        let mut original = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } == -1 {
            return Err(io::Error::last_os_error());
        }
        let mut terminal = Self {
            original,
            signals: Vec::new(),
            keyboard_depth,
        };
        for number in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT, libc::SIGQUIT] {
            terminal.signals.push(signal_hook::flag::register_usize(
                number,
                signal.clone(),
                number as usize,
            )?);
        }
        let mut raw = original;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } == -1 {
            return Err(io::Error::last_os_error());
        }
        io::stdout().write_all(b"\x1b[?1049h\x1b[2J\x1b[H")?;
        io::stdout().flush()?;
        Ok(terminal)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let depth = self.keyboard_depth.load(Ordering::Relaxed);
        if depth != 0 {
            let _ = write!(io::stdout(), "\x1b[<{depth}u");
        }
        let _ = io::stdout().write_all(
            b"\x1b[0m\x1b[?25h\x1b[?7h\x1b[?1l\x1b>\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1006l\x1b[?1049l",
        );
        let _ = io::stdout().flush();
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original) };
        for id in &self.signals {
            signal_hook::low_level::unregister(*id);
        }
    }
}

struct Child(Box<dyn portable_pty::Child + Send + Sync>);

impl Drop for Child {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[derive(Default)]
struct Callbacks {
    terminal: Vec<u8>,
    replies: Vec<u8>,
    keyboard_depth: Arc<AtomicUsize>,
}

impl Callbacks {
    fn osc(&mut self, params: &[&[u8]]) {
        self.terminal.extend_from_slice(b"\x1b]");
        for (index, param) in params.iter().enumerate() {
            if index != 0 {
                self.terminal.push(b';');
            }
            self.terminal.extend_from_slice(param);
        }
        self.terminal.extend_from_slice(b"\x1b\\");
    }
}

impl vt100::Callbacks for Callbacks {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.terminal.push(7);
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.osc(&[b"2", title]);
    }

    fn set_window_icon_name(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.osc(&[b"1", title]);
    }

    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8], data: &[u8]) {
        self.osc(&[b"52", ty, data]);
    }

    fn paste_from_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8]) {
        self.osc(&[b"52", ty, b"?"]);
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        self.osc(params);
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        if c == 'u' && i1 == Some(b'>') {
            self.keyboard_depth.fetch_add(1, Ordering::Relaxed);
        } else if c == 'u' && i1 == Some(b'<') {
            let _ = self
                .keyboard_depth
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |depth| {
                    Some(depth.saturating_sub(usize::from(first.max(1))))
                });
        }
        if i1.is_none() && c == 'n' && first == 6 {
            let (row, col) = screen.cursor_position();
            self.replies
                .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
        } else if i1.is_none() && c == 'n' && first == 5 {
            self.replies.extend_from_slice(b"\x1b[0n");
        } else if i1.is_none() && c == 't' && first == 18 {
            let (rows, cols) = screen.size();
            self.replies
                .extend_from_slice(format!("\x1b[8;{rows};{cols}t").as_bytes());
        } else if i1 == Some(b'?') && matches!(c, 'h' | 'l') && first != 1004 {
            // Private display modes cannot clear or scroll over APPA's footer.
        } else {
            let parameters = params
                .iter()
                .map(|group| group.iter().map(u16::to_string).collect::<Vec<_>>().join(":"))
                .collect::<Vec<_>>()
                .join(";");
            self.terminal.extend_from_slice(b"\x1b[");
            if let Some(prefix) = i1.filter(|i| (b'<'..=b'?').contains(i)) {
                self.terminal.push(prefix);
            }
            self.terminal.extend_from_slice(parameters.as_bytes());
            if let Some(intermediate) = i1.filter(|i| !(b'<'..=b'?').contains(i)) {
                self.terminal.push(intermediate);
            }
            if let Some(intermediate) = i2 {
                self.terminal.push(intermediate);
            }
            self.terminal.extend_from_slice(c.to_string().as_bytes());
        }
    }
}

fn clipped(text: &str, columns: u16) -> String {
    let mut width = 0;
    text.chars()
        .filter(|c| !c.is_control())
        .take_while(|c| {
            width += c.width().unwrap_or(0);
            width <= usize::from(columns)
        })
        .collect()
}

fn draw(
    output: &mut impl Write,
    parser: &vt100::Parser<Callbacks>,
    previous: Option<&vt100::Screen>,
    size: PtySize,
    chips: Option<&str>,
) -> io::Result<()> {
    let screen = parser.screen();
    output.write_all(&match previous {
        Some(previous) => screen.state_diff(previous),
        None => screen.state_formatted(),
    })?;
    let top = chips.map_or_else(|| MARK_TOP.to_owned(), |chips| format!("{MARK_TOP}  {chips}"));
    // Disable wrapping so a narrow terminal cannot scroll the child viewport.
    output.write_all(b"\x1b[0m\x1b[?7l")?;
    if size.rows > FOOTER_ROWS {
        write!(
            output,
            "\x1b[{};1H\x1b[2K{}\x1b[{};1H\x1b[2K{}",
            size.rows - 1,
            clipped(&top, size.cols),
            size.rows,
            clipped(MARK_BOTTOM, size.cols)
        )?;
    }
    output.write_all(b"\x1b[?7h")?;
    output.write_all(&screen.cursor_state_formatted())?;
    output.write_all(&screen.attributes_formatted())?;
    output.flush()
}

pub(crate) fn run(command: &Command, url: &str) -> Result<u8, String> {
    run_inner(command, url).map_err(|error| format!("Codex terminal wrapper failed: {error}"))
}

fn run_inner(command: &Command, url: &str) -> Result<u8, Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let status_path = directory.path().join("session.json");
    let mut builder = CommandBuilder::new(command.get_program());
    builder.args(command.get_args());
    if let Some(cwd) = command.get_current_dir() {
        builder.cwd(cwd);
    }
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            builder.env(key, value);
        } else {
            builder.env_remove(key);
        }
    }
    builder.env(STATUS_FILE, &status_path);
    let mut size = terminal_size()?;
    let pair = portable_pty::native_pty_system().openpty(child_size(size))?;
    let fd = pair.master.as_raw_fd().ok_or("the PTY has no file descriptor")?;
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let signal = Arc::new(AtomicUsize::new(0));
    let keyboard_depth = Arc::new(AtomicUsize::new(0));
    let _terminal = Terminal::enter(&signal, keyboard_depth.clone())?;
    let mut child = Child(pair.slave.spawn_command(builder)?);
    drop(pair.slave);
    let updates = status_updates(url.to_owned(), status_path);
    let mut chips = None;
    let mut parser = vt100::Parser::new_with_callbacks(
        child_size(size).rows,
        size.cols,
        0,
        Callbacks {
            keyboard_depth,
            ..Callbacks::default()
        },
    );
    let mut previous = None;
    let mut output = io::stdout();
    let mut buffer = [0; 16384];
    let mut dirty = true;
    let mut eof = false;
    let mut exited_at = None;
    loop {
        let interrupted = signal.load(Ordering::Relaxed);
        if interrupted != 0 {
            return Ok((128 + interrupted).min(255) as u8);
        }
        let next_size = terminal_size()?;
        if next_size.rows != size.rows || next_size.cols != size.cols {
            size = next_size;
            pair.master.resize(child_size(size))?;
            parser.screen_mut().set_size(child_size(size).rows, size.cols);
            previous = None;
            dirty = true;
            output.write_all(b"\x1b[2J")?;
        }
        while let Ok(next) = updates.try_recv() {
            dirty |= chips != next;
            chips = next;
        }
        if dirty {
            draw(&mut output, &parser, previous.as_ref(), size, chips.as_deref())?;
            previous = Some(parser.screen().clone());
            dirty = false;
        }
        if let Some(status) = child.0.try_wait()? {
            // Drain terminal output after exit before the guard restores the terminal.
            let exited = exited_at.get_or_insert_with(std::time::Instant::now);
            if eof || exited.elapsed() >= Duration::from_millis(300) {
                return Ok(status.exit_code().min(255) as u8);
            }
        }
        let mut poll = [
            libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        if unsafe { libc::poll(poll.as_mut_ptr(), poll.len() as libc::nfds_t, 30) } == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if poll[0].revents & libc::POLLIN != 0 {
            let length = io::stdin().read(&mut buffer)?;
            if length == 0 {
                return Ok(1);
            }
            writer.write_all(&buffer[..length])?;
        }
        if !eof && poll[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            match reader.read(&mut buffer) {
                Ok(0) => eof = true,
                Err(error) if error.raw_os_error() == Some(libc::EIO) => eof = true,
                Err(error) => return Err(error.into()),
                Ok(length) => {
                    parser.process(&buffer[..length]);
                    let callbacks = parser.callbacks_mut();
                    writer.write_all(&std::mem::take(&mut callbacks.replies))?;
                    output.write_all(&std::mem::take(&mut callbacks.terminal))?;
                    dirty = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_interactive_commands_get_a_footer() {
        for args in [
            vec![],
            vec!["resume"],
            vec!["fork"],
            vec!["-c", "model='exec'", "hello"],
            vec!["--", "--help"],
        ] {
            assert!(interactive_arguments(
                &args.into_iter().map(OsString::from).collect::<Vec<_>>()
            ));
        }
        for args in [
            vec!["exec", "hello"],
            vec!["--help"],
            vec!["-C", "/tmp", "e", "hello"],
            vec!["mcp", "list"],
            vec!["review"],
        ] {
            assert!(!interactive_arguments(
                &args.into_iter().map(OsString::from).collect::<Vec<_>>()
            ));
        }
    }

    #[test]
    fn child_clear_and_alternate_screen_leave_the_footer_and_cursor_intact() {
        let size = PtySize {
            rows: 6,
            cols: 60,
            ..PtySize::default()
        };
        let mut child = vt100::Parser::new_with_callbacks(4, 60, 0, Callbacks::default());
        let mut terminal = vt100::Parser::new(6, 60, 0);
        child.process(b"\x1b[?1049h\x1b[2J\x1b[3;2HCodex\x1b[?2004h");
        let mut bytes = Vec::new();
        draw(&mut bytes, &child, None, size, Some("trust:trusted  audience:internal")).unwrap();
        terminal.process(&bytes);
        assert!(
            terminal
                .screen()
                .contents()
                .contains("trust:trusted  audience:internal")
        );
        assert_eq!(terminal.screen().cursor_position(), child.screen().cursor_position());
        assert!(terminal.screen().bracketed_paste());
        let previous = child.screen().clone();
        child.process(b"\x1b[2J\x1b[Hnew");
        bytes.clear();
        draw(&mut bytes, &child, Some(&previous), size, None).unwrap();
        terminal.process(&bytes);
        assert!(terminal.screen().contents().contains("new"));
        assert!(terminal.screen().contents().contains(MARK_BOTTOM));
        assert!(!terminal.screen().contents().contains("trust:"));
    }

    #[test]
    fn queries_use_the_child_viewport_and_forward_terminal_capabilities() {
        let mut parser = vt100::Parser::new_with_callbacks(10, 40, 0, Callbacks::default());
        parser.process(b"\x1b[3;4H\x1b[6n\x1b[18t\x1b[?u\x1b]11;?\x07");
        assert_eq!(parser.callbacks().replies, b"\x1b[3;4R\x1b[8;10;40t");
        assert_eq!(parser.callbacks().terminal, b"\x1b[?0u\x1b]11;?\x1b\\");
    }

    #[test]
    fn cleanup_pops_only_keyboard_modes_that_the_child_left_active() {
        let mut parser = vt100::Parser::new_with_callbacks(10, 40, 0, Callbacks::default());
        parser.process(b"\x1b[>1u\x1b[>3u\x1b[<u\x1b[?1004h\x1b[?2026h");
        assert_eq!(parser.callbacks().keyboard_depth.load(Ordering::Relaxed), 1);
        assert!(parser.callbacks().terminal.ends_with(b"\x1b[?1004h"));
        parser.process(b"\x1b[<u");
        assert_eq!(parser.callbacks().keyboard_depth.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn narrow_footer_does_not_wrap_or_emit_controls() {
        assert_eq!(clipped("a界b", 3), "a界");
        assert_eq!(clipped("a\n\x1bb", 2), "ab");
        assert_eq!(
            child_size(PtySize {
                rows: 1,
                ..PtySize::default()
            })
            .rows,
            1
        );
    }
}
