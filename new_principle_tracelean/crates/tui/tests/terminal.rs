use std::ffi::c_int;
use std::io::Read;
use std::os::unix::io::FromRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

const QUIET_MS: c_int = 250;
const MOST_READS: usize = 400;

pub struct Terminal {
    master: std::fs::File,
    child: Child,
    pending: String,
}

impl Terminal {
    pub fn open(root: &Path, rows: u16, columns: u16) -> Terminal {
        let mut master: c_int = -1;
        let mut slave: c_int = -1;
        let size = libc::winsize {
            ws_row: rows,
            ws_col: columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let opened = unsafe {
            libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &size)
        };
        assert_eq!(opened, 0, "no pseudo-terminal: {}", std::io::Error::last_os_error());

        let (stdin, stdout, stderr) = unsafe {
            (
                Stdio::from_raw_fd(libc::dup(slave)),
                Stdio::from_raw_fd(libc::dup(slave)),
                Stdio::from_raw_fd(libc::dup(slave)),
            )
        };

        let mut command = Command::new(env!("CARGO_BIN_EXE_tracelean-tui"));
        command.arg("--fresh").arg(root).stdin(stdin).stdout(stdout).stderr(stderr);
        unsafe {
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

        unsafe { libc::close(slave) };
        let master = unsafe { std::fs::File::from_raw_fd(master) };

        let mut terminal = Terminal { master, child, pending: String::new() };
        terminal.settle();
        terminal
    }

    pub fn press(&mut self, bytes: &str) -> Vec<String> {
        use std::io::Write;
        self.master.write_all(bytes.as_bytes()).expect("the terminal accepts a key");
        self.master.flush().expect("the terminal accepts a key");
        self.settle();
        self.screen()
    }

    #[allow(dead_code)]
    pub fn painted(&self) -> String {
        let at = self.pending.rfind("\u{1b}[2J").unwrap_or(0);
        self.pending[at..].to_string()
    }

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

    fn settle(&mut self) {
        for _ in 0..MOST_READS {
            if !self.waiting() {
                return;
            }
            let mut buffer = [0u8; 65_536];
            match self.master.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => self.pending.push_str(&String::from_utf8_lossy(&buffer[..read])),
                Err(_) => return,
            }
        }
        panic!("the frontend painted {MOST_READS} times without going quiet");
    }

    fn waiting(&self) -> bool {
        use std::os::unix::io::AsRawFd;
        let mut asked = libc::pollfd { fd: self.master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let ready = unsafe { libc::poll(&mut asked, 1, QUIET_MS) };
        ready > 0
    }

    pub fn finished(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
