//! `tracelean-sandbox` — enter a session's sandbox from the command line.
//!
//! tracelean never launches Claude Code (or any external agent) itself —
//! the user runs this binary in their own terminal to get a shell inside
//! the session's `bwrap` namespace, then runs whatever they want in there
//! exactly as they would outside it. tracelean's GUI only creates the
//! session and watches its work dir (see `tracelean_core::sandbox`); this
//! binary is the only thing that actually enters the namespace, and it can
//! be run any number of times against the same session.
//!
//! Usage:
//!   tracelean-sandbox shell <project-root> <session-id> [-- <command>...]
//!
//! With no trailing command, execs `$SHELL` (falling back to `/bin/sh`).

use std::env;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command as StdCommand;

use tracelean_core::sandbox::session::{bwrap_argv, bwrap_available, load_session};

fn usage() -> ! {
    eprintln!("usage: tracelean-sandbox shell <project-root> <session-id> [-- <command>...]");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 || args[1] != "shell" {
        usage();
    }
    let project_root = PathBuf::from(&args[2]);
    let session_id = &args[3];

    if !bwrap_available() {
        eprintln!("tracelean-sandbox: bwrap not found on PATH");
        std::process::exit(1);
    }

    let spec = match load_session(&project_root, session_id) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tracelean-sandbox: {}", e);
            std::process::exit(1);
        }
    };

    let mut argv = bwrap_argv(&spec);

    let mut trailing: Vec<String> = args.into_iter().skip(4).collect();
    if trailing.first().map(|a| a == "--").unwrap_or(false) {
        trailing.remove(0);
    }
    if trailing.is_empty() {
        argv.push(env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()));
    } else {
        argv.extend(trailing);
    }

    // exec replaces this process — no wrapper process is left running once
    // the shell starts, so this behaves like an ordinary terminal session.
    let err = StdCommand::new("bwrap").args(&argv).exec();
    eprintln!("tracelean-sandbox: failed to exec bwrap: {}", err);
    std::process::exit(1);
}
