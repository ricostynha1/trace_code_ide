//! A terminal, for the length of one session.
//!
//! The driving suite (`REQ-DRIVE`) needs the one thing a pipe cannot give it: a
//! frontend that believes it is talking to a person. `crossterm` reads a
//! terminal, not standard input, so a harness that fed the binary a pipe would
//! be testing a code path nobody runs — and the bug this requirement was
//! written for lived in the code that turns a byte into a key name, which a
//! pipe skips entirely.
//!
//! So this opens a pseudo-terminal and spawns the real binary on it. What is
//! written here is what a keyboard writes; what is read back is what a terminal
//! would have received. Nothing is mocked and nothing is named on the
//! frontend's behalf.
//!
//! `libc` is a development dependency and already in the lock through
//! `crossterm`, so the harness vendors nothing new.
//!
//! Realises REQ-DRIVE.keys_arrive_as_bytes and
//! REQ-DRIVE.screen_answers_for_the_frontend. The claims are annotated in
//! `driving.rs`, which is the file that states them as tests.

use std::ffi::c_int;
use std::io::Read;
use std::os::unix::io::FromRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// How long to wait for the frontend to finish painting before deciding it has
/// nothing more to say, and how many times to ask.
///
/// A wait, not a clock: nothing here reads the time, so a session is still a
/// function of the keys it was given (`ARCH-DETERMINISM`). The ceiling is what
/// turns a frontend that never answers into a failed test rather than a hung
/// one.
const QUIET_MS: c_int = 250;
const MOST_READS: usize = 400;

/// A running frontend and the terminal it thinks it has.
pub struct Terminal {
    master: std::fs::File,
    child: Child,
    /// Everything read so far and not yet consumed as a screen.
    pending: String,
}

impl Terminal {
    /// Open a terminal of the given size and run the frontend on the tree.
    ///
    /// The size is fixed rather than inherited: how many lines the frontend has
    /// room for decides what it draws, and a suite whose expectations depended
    /// on the window somebody ran it in would pass on one machine.
    pub fn open(root: &Path, rows: u16, columns: u16) -> Terminal {
        let mut master: c_int = -1;
        let mut slave: c_int = -1;
        let size = libc::winsize {
            ws_row: rows,
            ws_col: columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: both file descriptors are out-parameters, and the window size
        // is a local this call only reads.
        let opened = unsafe {
            libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &size)
        };
        assert_eq!(opened, 0, "no pseudo-terminal: {}", std::io::Error::last_os_error());

        // SAFETY: `slave` is a fresh descriptor this function owns, duplicated
        // three times so the child's standard streams all name the terminal.
        let (stdin, stdout, stderr) = unsafe {
            (
                Stdio::from_raw_fd(libc::dup(slave)),
                Stdio::from_raw_fd(libc::dup(slave)),
                Stdio::from_raw_fd(libc::dup(slave)),
            )
        };

        let mut command = Command::new(env!("CARGO_BIN_EXE_tracelean-tui"));
        // Fresh: the demo tree is shared, so what one run had open must not
        // be what the next one opens on.
        command.arg("--fresh").arg(root).stdin(stdin).stdout(stdout).stderr(stderr);
        unsafe {
            // A session of its own, with this terminal as its controlling one —
            // which is what makes the child a foreground process a terminal
            // delivers keys to, rather than one that is merely holding the
            // descriptor.
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("the terminal frontend runs");

        // SAFETY: the parent's copy of the slave end is finished with — held
        // open it would keep the master readable forever, so the harness could
        // not tell a frontend that exited from one that went quiet.
        unsafe { libc::close(slave) };
        // SAFETY: `master` is owned from here, and closed when the file drops.
        let master = unsafe { std::fs::File::from_raw_fd(master) };

        let mut terminal = Terminal { master, child, pending: String::new() };
        // Whatever it painted before anybody pressed anything.
        terminal.settle();
        terminal
    }

    /// Press keys and answer with the screen they left.
    ///
    /// The bytes are what a keyboard sends: `" "` for the space bar, `\x1b` for
    /// escape. Nothing here knows what the frontend will call them.
    pub fn press(&mut self, bytes: &str) -> Vec<String> {
        use std::io::Write;
        self.master.write_all(bytes.as_bytes()).expect("the terminal accepts a key");
        self.master.flush().expect("the terminal accepts a key");
        self.settle();
        self.screen()
    }

    /// The bytes of the last thing painted, escapes and all: how it is drawn,
    /// not only what it says.
    #[allow(dead_code)]
    pub fn painted(&self) -> String {
        let at = self.pending.rfind("\u{1b}[2J").unwrap_or(0);
        self.pending[at..].to_string()
    }

    /// The screen as it stands: the last thing painted, read as the text a
    /// person would have read.
    ///
    /// A frontend repaints the whole screen, clearing it first, so the last
    /// clear is where the current screen begins. Everything before it is what
    /// was on the screen a key press ago.
    pub fn screen(&self) -> Vec<String> {
        let latest = match self.pending.rfind("\u{1b}[2J") {
            Some(at) => &self.pending[at..],
            None => self.pending.as_str(),
        };
        tracelean_core::drt::frontend::readable(latest)
            .unwrap_or_default()
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect()
    }

    /// Read until the frontend stops saying anything.
    fn settle(&mut self) {
        for _ in 0..MOST_READS {
            if !self.waiting() {
                return;
            }
            let mut buffer = [0u8; 65_536];
            match self.master.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => self.pending.push_str(&String::from_utf8_lossy(&buffer[..read])),
                // The frontend exited and took the terminal with it, which is an
                // answer: whatever it painted last is on the screen.
                Err(_) => return,
            }
        }
        panic!("the frontend painted {MOST_READS} times without going quiet");
    }

    /// Whether there is anything to read, waiting briefly rather than spinning.
    fn waiting(&self) -> bool {
        use std::os::unix::io::AsRawFd;
        let mut asked = libc::pollfd { fd: self.master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one descriptor this harness owns, and a count that matches.
        let ready = unsafe { libc::poll(&mut asked, 1, QUIET_MS) };
        ready > 0
    }

    /// Whether the frontend has exited.
    pub fn finished(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // A test that failed mid-session leaves a frontend sitting in raw mode
        // on a terminal nobody is reading. Ending it here is what keeps one
        // failure from being every later test timing out.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
