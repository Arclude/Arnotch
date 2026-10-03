//! Finds the user's open terminals and what runs in them, by reading `/proc`.
//!
//! Every terminal tab runs a shell that leads its own session on a pty. The kernel
//! records the foreground process group of that pty (`tpgid` in `/proc/<pid>/stat`),
//! which is the command currently running in the tab. This works with any terminal
//! emulator, without its cooperation.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const SHELLS: &[&str] = &[
    "bash", "zsh", "fish", "sh", "dash", "ksh", "mksh", "tcsh", "csh", "nu", "elvish", "xonsh",
    "ion",
];

/// Process names of terminal emulators (and multiplexers), mapped to a display name.
const TERMINAL_APPS: &[(&str, &str)] = &[
    ("konsole", "Konsole"),
    ("kitty", "kitty"),
    ("wezterm-gui", "WezTerm"),
    ("alacritty", "Alacritty"),
    ("foot", "foot"),
    ("ghostty", "Ghostty"),
    ("gnome-terminal-", "GNOME Terminal"),
    ("ptyxis-agent", "Ptyxis"),
    ("kgx", "Console"),
    ("tilix", "Tilix"),
    ("terminator", "Terminator"),
    ("xfce4-terminal", "Xfce Terminal"),
    ("cosmic-term", "COSMIC Terminal"),
    ("yakuake", "Yakuake"),
    ("xterm", "xterm"),
    ("urxvt", "urxvt"),
    ("st", "st"),
    ("warp", "Warp"),
    ("arterm", "Arterm"),
    ("code", "VS Code"),
];

/// Full-screen or long-lived interactive programs: their exit is not news.
const INTERACTIVE: &[&str] = &[
    "vim",
    "nvim",
    "vi",
    "nano",
    "emacs",
    "hx",
    "helix",
    "micro",
    "less",
    "more",
    "man",
    "htop",
    "btop",
    "top",
    "ssh",
    "mosh",
    "tmux",
    "screen",
    "zellij",
    "claude",
    "python",
    "python3",
    "node",
    "irb",
    "ghci",
    "mc",
    "ranger",
    "yazi",
    "lf",
    "nnn",
    "watch",
    "journalctl",
];

/// Commands running at least this long announce when they finish.
const NOTIFY_AFTER_SECS: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Terminal {
    /// The pty, e.g. "pts/3". Stable for the lifetime of the tab.
    pub id: String,
    /// Display name of the terminal emulator, or "tmux".
    pub app: String,
    /// The emulator's process, used to focus its window. 0 when unknown.
    pub app_pid: u32,
    pub shell_pid: u32,
    pub shell: String,
    pub cwd: String,
    pub command: Option<Command>,
    /// Text can be typed into this terminal (see [`crate::control`]).
    pub can_send: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Command {
    /// Foreground process group; signals go to the whole group, like Ctrl+C.
    pub pgid: i32,
    pub name: String,
    pub args: String,
    /// Unix time in seconds.
    pub started_at: u64,
    /// Percent of one CPU core since the previous scan.
    pub cpu: f32,
}

/// A command that ran for a while and has just ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub app: String,
    pub cwd: String,
    pub args: String,
    pub duration_secs: u64,
}

#[derive(Debug, Clone)]
struct Proc {
    pid: i32,
    ppid: i32,
    pgrp: i32,
    session: i32,
    tty_nr: i32,
    tpgid: i32,
    comm: String,
    ticks: u64,
    start_ticks: u64,
}

pub struct Scanner {
    uid: u32,
    clock_ticks: u64,
    boot_time: u64,
    /// CPU ticks per foreground group at the previous scan.
    prev_ticks: HashMap<i32, u64>,
    prev_scan: Option<Instant>,
    /// What each pty was running at the previous scan.
    running: HashMap<String, (Command, String, String)>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            // SAFETY: getuid and sysconf have no preconditions.
            uid: unsafe { libc::getuid() },
            clock_ticks: unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64,
            boot_time: boot_time().unwrap_or(0),
            prev_ticks: HashMap::new(),
            prev_scan: None,
            running: HashMap::new(),
        }
    }

    /// Lists terminals and reports the commands that finished since the last scan.
    pub fn scan(&mut self) -> (Vec<Terminal>, Vec<Finished>) {
        let procs = self.read_procs();
        let by_pid: HashMap<i32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();

        let now = Instant::now();
        let elapsed = self
            .prev_scan
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(0.0);
        self.prev_scan = Some(now);

        let mut group_ticks: HashMap<i32, u64> = HashMap::new();
        for p in &procs {
            *group_ticks.entry(p.pgrp).or_default() += p.ticks;
        }

        let mut terminals: Vec<Terminal> = procs
            .iter()
            .filter(|p| p.pid == p.session && p.tty_nr != 0 && SHELLS.contains(&p.comm.as_str()))
            .map(|shell| {
                let (app, app_pid) = find_app(shell, &by_pid);
                let command = (shell.tpgid > 0 && shell.tpgid != shell.pgrp)
                    .then(|| self.command(shell.tpgid, &by_pid, &group_ticks, elapsed))
                    .flatten();
                let cwd_pid = command.as_ref().map_or(shell.pid, |c| c.pgid);
                Terminal {
                    id: pty_name(shell.tty_nr),
                    can_send: crate::control::can_send(&app, app_pid),
                    app,
                    app_pid,
                    shell_pid: shell.pid as u32,
                    shell: shell.comm.clone(),
                    cwd: cwd_of(cwd_pid)
                        .or_else(|| cwd_of(shell.pid))
                        .unwrap_or_default(),
                    command,
                }
            })
            .collect();
        terminals.sort_by(|a, b| {
            a.app
                .cmp(&b.app)
                .then(pty_number(&a.id).cmp(&pty_number(&b.id)))
        });

        self.prev_ticks = group_ticks;
        let finished = self.track_finished(&terminals);
        (terminals, finished)
    }

    fn command(
        &self,
        pgid: i32,
        by_pid: &HashMap<i32, &Proc>,
        group_ticks: &HashMap<i32, u64>,
        elapsed: f32,
    ) -> Option<Command> {
        let leader = by_pid.get(&pgid)?;
        let ticks = group_ticks.get(&pgid).copied().unwrap_or(0);
        let cpu = match self.prev_ticks.get(&pgid) {
            Some(&prev) if elapsed > 0.0 => {
                (ticks.saturating_sub(prev) as f32 / self.clock_ticks as f32) / elapsed * 100.0
            }
            _ => 0.0,
        };
        Some(Command {
            pgid,
            name: leader.comm.clone(),
            args: cmdline(pgid).unwrap_or_else(|| leader.comm.clone()),
            started_at: self.boot_time + leader.start_ticks / self.clock_ticks,
            cpu: (cpu * 10.0).round() / 10.0,
        })
    }

    fn track_finished(&mut self, terminals: &[Terminal]) -> Vec<Finished> {
        let now = unix_now();
        let mut finished = Vec::new();
        let mut running = HashMap::new();

        for t in terminals {
            if let Some(command) = &t.command {
                running.insert(
                    t.id.clone(),
                    (command.clone(), t.app.clone(), t.cwd.clone()),
                );
            }
        }
        for (id, (command, app, cwd)) in self.running.drain() {
            let same = running
                .get(&id)
                .is_some_and(|(c, ..)| c.pgid == command.pgid);
            let duration_secs = now.saturating_sub(command.started_at);
            if !same
                && duration_secs >= NOTIFY_AFTER_SECS
                && !INTERACTIVE.contains(&command.name.as_str())
            {
                finished.push(Finished {
                    app,
                    cwd,
                    args: command.args,
                    duration_secs,
                });
            }
        }
        self.running = running;
        finished
    }

    fn read_procs(&self) -> Vec<Proc> {
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
            })
            .filter(|e| e.metadata().is_ok_and(|m| m.uid() == self.uid))
            .filter_map(|e| parse_stat(&fs::read_to_string(e.path().join("stat")).ok()?))
            .collect()
    }
}

/// Parses `/proc/<pid>/stat`. The command name is in parentheses and may contain spaces.
fn parse_stat(stat: &str) -> Option<Proc> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let pid = stat[..open].trim().parse().ok()?;
    let comm = stat[open + 1..close].to_owned();
    // Fields after the name, starting with field 3 (state).
    let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
    let field = |n: usize| fields.get(n - 3).and_then(|f| f.parse::<i64>().ok());

    Some(Proc {
        pid,
        comm,
        ppid: field(4)? as i32,
        pgrp: field(5)? as i32,
        session: field(6)? as i32,
        tty_nr: field(7)? as i32,
        tpgid: field(8)? as i32,
        ticks: (field(14)? + field(15)?) as u64,
        start_ticks: field(22)? as u64,
    })
}

/// Walks up from the shell to the terminal emulator (or tmux) that owns it.
fn find_app(shell: &Proc, by_pid: &HashMap<i32, &Proc>) -> (String, u32) {
    let mut pid = shell.ppid;
    for _ in 0..12 {
        let Some(p) = by_pid.get(&pid) else { break };
        if p.comm.starts_with("tmux") {
            return ("tmux".into(), p.pid as u32);
        }
        if let Some((_, name)) = TERMINAL_APPS.iter().find(|(comm, _)| p.comm == *comm) {
            // Some emulators run a helper per tab; keep walking to the real app.
            let app_pid = by_pid
                .get(&p.ppid)
                .filter(|parent| parent.comm == p.comm)
                .map_or(p.pid, |parent| parent.pid);
            return ((*name).into(), app_pid as u32);
        }
        pid = p.ppid;
    }
    ("Terminal".into(), 0)
}

/// The terminal emulator that process `pid` runs in, read straight from `/proc`.
pub fn terminal_app_of(pid: i32) -> Option<(String, u32)> {
    let mut by_pid = HashMap::new();
    let mut next = pid;
    for _ in 0..16 {
        let proc = parse_stat(&fs::read_to_string(format!("/proc/{next}/stat")).ok()?)?;
        next = proc.ppid;
        by_pid.insert(proc.pid, proc);
        if next <= 1 {
            break;
        }
    }
    let start = by_pid.get(&pid)?.clone();
    let refs = by_pid.iter().map(|(pid, p)| (*pid, p)).collect();
    match find_app(&start, &refs) {
        (_, 0) => None,
        found => Some(found),
    }
}

/// "pts/3" from a `tty_nr` (major 136..=143 are Unix98 ptys).
fn pty_name(tty_nr: i32) -> String {
    let major = (tty_nr >> 8) & 0xfff;
    let minor = (tty_nr & 0xff) | ((tty_nr >> 12) & 0xfff00);
    if (136..=143).contains(&major) {
        format!("pts/{}", (major - 136) * 256 + minor)
    } else {
        format!("tty{minor}")
    }
}

fn pty_number(id: &str) -> u32 {
    id.rsplit('/')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn cmdline(pid: i32) -> Option<String> {
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = raw
        .split(|&b| b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    let mut line = args.join(" ");
    if line.is_empty() {
        return None;
    }
    if let Some((end, _)) = line.char_indices().nth(120) {
        line.truncate(end);
        line.push('…');
    }
    Some(line)
}

fn cwd_of(pid: i32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/cwd"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

fn boot_time() -> Option<u64> {
    fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Stops the command running in a terminal. The first call sends SIGINT, like Ctrl+C;
/// calling again within a few seconds sends SIGTERM, for programs that ignore Ctrl+C.
/// The foreground group is looked up again so a stale list can't hit another process.
pub fn interrupt(shell_pid: u32) -> std::io::Result<()> {
    static LAST: Mutex<Option<(i32, Instant)>> = Mutex::new(None);
    const ESCALATE_WITHIN: Duration = Duration::from_secs(10);

    let stat = fs::read_to_string(format!("/proc/{shell_pid}/stat"))?;
    let shell = parse_stat(&stat).ok_or_else(|| std::io::Error::other("unreadable shell stat"))?;
    let pgid = shell.tpgid;
    if pgid <= 0 || pgid == shell.pgrp {
        return Err(std::io::Error::other("nothing is running"));
    }

    let mut last = LAST.lock().unwrap();
    let signal = match *last {
        Some((group, at)) if group == pgid && at.elapsed() < ESCALATE_WITHIN => libc::SIGTERM,
        _ => libc::SIGINT,
    };
    *last = Some((pgid, Instant::now()));

    // SAFETY: kill has no memory-safety preconditions; a negative pid targets the group.
    if unsafe { libc::kill(-pgid, signal) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_with_spaces_in_the_name() {
        let stat = "1234 (tmux: server) S 1 1234 1234 34817 5678 4194560 0 0 0 0 15 7 0 0 20 0 1 0 98765 0 0";
        let p = parse_stat(stat).unwrap();
        assert_eq!(p.comm, "tmux: server");
        assert_eq!((p.pid, p.ppid, p.pgrp, p.session), (1234, 1, 1234, 1234));
        assert_eq!(
            (p.tty_nr, p.tpgid, p.ticks, p.start_ticks),
            (34817, 5678, 22, 98765)
        );
    }

    #[test]
    fn names_ptys() {
        assert_eq!(pty_name(34816), "pts/0");
        assert_eq!(pty_name(34819), "pts/3");
    }

    #[test]
    fn finds_this_process_tree_without_panicking() {
        let mut scanner = Scanner::new();
        scanner.scan();
        scanner.scan();
    }
}
