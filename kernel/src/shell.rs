//! Tiny line-oriented shell.
//!
//! The shell is split between a **pure** core (`Shell::execute_line`) and an
//! **async driver** (`shell_loop`). The pure core takes a command line as a
//! string and returns the rendered output as a string — no I/O, no IRQs, no
//! globals. Tests exercise it directly without a keyboard. The driver owns the
//! keyboard `ScancodeStream`, builds a line buffer character-by-character, and
//! mirrors output to both the framebuffer (for the QEMU/VBox window) and the
//! serial console (for headless and tests).
//!
//! M10 changes:
//! - Backed by a `Vfs` (multi-mount) instead of a single `Fs`.
//! - Framebuffer scrollback is real; `PgUp`/`PgDn`/`End` route through
//!   `task::keyboard::route_rawkey`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use futures_util::stream::StreamExt;
use pc_keyboard::{DecodedKey, HandleControl, Keyboard, ScancodeSet1, layouts};

use crate::fs::{self, FsError, Vfs};
use crate::task::keyboard::{self, ScancodeStream};

const HELP_TEXT: &str = "\
willo shell — built-in commands:
  pwd                  print working directory
  ls [path]            list entries in path (or cwd)
  cd <path>            change directory
  cat <path>           print file contents
  echo <text>          echo arguments
  clear                clear the screen
  mount                show what is mounted at /
  uptime               show timer ticks since boot
  touch <path>         create empty file (or truncate)
  mkdir <path>         create directory
  rm <path>            delete file or empty directory
  write <path> <text>  overwrite file with text
  help                 show this help
";

/// Sentinel returned from `execute_line` to ask the driver to clear the screen.
const CLEAR_SENTINEL: &str = "\x1bCLR";

pub struct Shell {
    vfs: Vfs,
    cwd: String,
}

impl Shell {
    pub fn new(vfs: Vfs) -> Self {
        Self {
            vfs,
            cwd: "/".to_string(),
        }
    }

    pub fn prompt(&self) -> String {
        format!("willo:{}$ ", self.cwd)
    }

    /// Run a single command line and return the output string (always either
    /// empty or `\n`-terminated). Pure: no I/O, no global state outside `self`.
    pub fn execute_line(&mut self, line: &str) -> String {
        let mut parts = line.split_ascii_whitespace();
        let cmd = match parts.next() {
            Some(c) => c,
            None => return String::new(),
        };
        let args: Vec<&str> = parts.collect();
        match cmd {
            "pwd" => format!("{}\n", self.cwd),
            "ls" => self.cmd_ls(args.first().copied()),
            "cd" => self.cmd_cd(args.first().copied()),
            "cat" => self.cmd_cat(args.first().copied()),
            "echo" => {
                let mut out = args.join(" ");
                out.push('\n');
                out
            }
            "clear" => CLEAR_SENTINEL.to_string(),
            "mount" => {
                let mut out = String::new();
                for (prefix, backing) in self.vfs.mounts() {
                    out.push_str(&format!("{prefix} on {backing}\n"));
                }
                out
            }
            "uptime" => format!("{} ticks\n", crate::interrupts::ticks()),
            "touch" => self.cmd_touch(args.first().copied()),
            "mkdir" => self.cmd_mkdir(args.first().copied()),
            "rm" => self.cmd_rm(args.first().copied()),
            "write" => self.cmd_write(&args),
            "help" => HELP_TEXT.to_string(),
            other => format!("willo: command not found: {other}\n"),
        }
    }

    fn cmd_ls(&self, path: Option<&str>) -> String {
        let target = match path {
            Some(p) => fs::normalize(&self.cwd, p),
            None => self.cwd.clone(),
        };
        match self.vfs.list_dir(&target) {
            Ok(entries) => {
                let mut sorted = entries;
                sorted.sort();
                let mut out = String::new();
                for name in &sorted {
                    let child = format!("{}/{}", target.trim_end_matches('/'), name);
                    out.push_str(name);
                    if self.vfs.is_dir(&child) {
                        out.push('/');
                    }
                    out.push('\n');
                }
                out
            }
            Err(FsError::NotFound) => format!("ls: not found: {target}\n"),
            Err(FsError::NotADir) => format!("ls: not a directory: {target}\n"),
            Err(FsError::IsADir) => unreachable!(),
            Err(FsError::Io) => format!("ls: i/o error: {target}\n"),
            Err(FsError::Unsupported) => format!("ls: unsupported: {target}\n"),
        }
    }

    fn cmd_cd(&mut self, path: Option<&str>) -> String {
        let p = match path {
            Some(p) => fs::normalize(&self.cwd, p),
            None => "/".to_string(),
        };
        if !self.vfs.is_dir(&p) {
            return format!("cd: not a directory: {p}\n");
        }
        self.cwd = p;
        String::new()
    }

    fn cmd_cat(&self, path: Option<&str>) -> String {
        let p = match path {
            Some(p) => fs::normalize(&self.cwd, p),
            None => return "cat: missing operand\n".to_string(),
        };
        match self.vfs.read_file(&p) {
            Ok(bytes) => {
                let s = match core::str::from_utf8(&bytes) {
                    Ok(s) => s.to_string(),
                    Err(_) => {
                        let mut tmp = String::with_capacity(bytes.len());
                        for b in bytes {
                            tmp.push(b as char);
                        }
                        tmp
                    }
                };
                if s.ends_with('\n') {
                    s
                } else {
                    let mut out = s;
                    out.push('\n');
                    out
                }
            }
            Err(FsError::NotFound) => format!("cat: not found: {p}\n"),
            Err(FsError::IsADir) => format!("cat: is a directory: {p}\n"),
            Err(FsError::NotADir) => format!("cat: not a directory: {p}\n"),
            Err(FsError::Io) => format!("cat: i/o error: {p}\n"),
            Err(FsError::Unsupported) => format!("cat: unsupported: {p}\n"),
        }
    }

    fn cmd_touch(&self, path: Option<&str>) -> String {
        let p = match path {
            Some(p) => fs::normalize(&self.cwd, p),
            None => return "touch: missing operand\n".to_string(),
        };
        match self.vfs.create_file(&p) {
            Ok(()) => String::new(),
            Err(FsError::Unsupported) => "touch: read-only filesystem\n".to_string(),
            Err(e) => format!("touch: {p}: {e:?}\n"),
        }
    }

    fn cmd_mkdir(&self, path: Option<&str>) -> String {
        let p = match path {
            Some(p) => fs::normalize(&self.cwd, p),
            None => return "mkdir: missing operand\n".to_string(),
        };
        match self.vfs.create_dir(&p) {
            Ok(()) => String::new(),
            Err(FsError::Unsupported) => "mkdir: read-only filesystem\n".to_string(),
            Err(e) => format!("mkdir: {p}: {e:?}\n"),
        }
    }

    fn cmd_rm(&self, path: Option<&str>) -> String {
        let p = match path {
            Some(p) => fs::normalize(&self.cwd, p),
            None => return "rm: missing operand\n".to_string(),
        };
        match self.vfs.remove(&p) {
            Ok(()) => String::new(),
            Err(FsError::Unsupported) => "rm: read-only filesystem\n".to_string(),
            Err(e) => format!("rm: {p}: {e:?}\n"),
        }
    }

    /// `write <path> <text...>` — joins remaining args with single spaces and
    /// appends a trailing newline before writing. Overwrites any existing file.
    fn cmd_write(&self, args: &[&str]) -> String {
        let path = match args.first() {
            Some(p) => fs::normalize(&self.cwd, p),
            None => return "write: missing operand: <path> <text...>\n".to_string(),
        };
        let mut content = args[1..].join(" ");
        content.push('\n');
        match self.vfs.write_file(&path, content.as_bytes()) {
            Ok(()) => String::new(),
            Err(FsError::Unsupported) => "write: read-only filesystem\n".to_string(),
            Err(e) => format!("write: {path}: {e:?}\n"),
        }
    }
}

/// Async driver: reads decoded keys, builds a line, dispatches on Enter.
///
/// Owns the single `ScancodeStream` (which can only be created once across
/// the whole kernel). Routes `DecodedKey::RawKey` to the framebuffer's
/// scrollback API (PgUp/PgDn/End) so history navigation never reaches the
/// shell's line buffer.
pub async fn shell_loop(vfs: Vfs) {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    );

    let mut shell = Shell::new(vfs);
    let mut line = String::with_capacity(128);

    let prompt = shell.prompt();
    crate::print!("{}", prompt);
    crate::serial_print!("{}", prompt);

    while let Some(scancode) = scancodes.next().await {
        let key = match keyboard.add_byte(scancode) {
            Ok(Some(event)) => keyboard.process_keyevent(event),
            _ => None,
        };
        let key = match key {
            Some(k) => k,
            None => continue,
        };
        match key {
            DecodedKey::RawKey(k) => keyboard::route_rawkey(k),
            DecodedKey::Unicode(c) => match c {
                '\n' => {
                    crate::print!("\n");
                    crate::serial_print!("\n");
                    let out = shell.execute_line(&line);
                    if out == CLEAR_SENTINEL {
                        if let Some(w) = crate::FB_WRITER.get() {
                            w.lock().clear();
                        }
                        crate::serial_print!("\x1b[2J\x1b[H");
                    } else if !out.is_empty() {
                        crate::print!("{}", out);
                        crate::serial_print!("{}", out);
                    }
                    line.clear();
                    let prompt = shell.prompt();
                    crate::print!("{}", prompt);
                    crate::serial_print!("{}", prompt);
                }
                '\x08' | '\x7f' => {
                    if line.pop().is_some() {
                        crate::print!("\u{8} \u{8}");
                        crate::serial_print!("\u{8} \u{8}");
                    }
                }
                c if (' '..='~').contains(&c) && line.len() < 120 => {
                    line.push(c);
                    crate::print!("{}", c);
                    crate::serial_print!("{}", c);
                }
                _ => {}
            },
        }
    }
}
