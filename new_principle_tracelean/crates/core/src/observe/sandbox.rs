//! A sandbox session, and the command that enters it.
//!
//! The editor never starts the agent (`ARCH-NO-DRIVING.user_runs_it`). What it
//! does is make a copy of the project and hand the user one line to paste into
//! their own terminal: a bubblewrap shell in which the copy sits at the
//! project's own path, so the agent sees the project where it expects it and
//! writes only into the copy.
//!
//! Pure: what exists on disk is gathered by the shell and passed in, so the
//! command is a function of facts and can be tested without a filesystem.

use serde::{Deserialize, Serialize};

/// One sandbox: where the project is, and where its copy is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    /// The real project, absolute.
    pub root: String,
    /// The copy the agent works in, absolute.
    pub work: String,
}

/// What the shell found on this machine that the command depends on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub home: Option<String>,
    /// Home-relative paths that exist and an agent needs to write: its own
    /// state (`.claude`, `.claude.json`) and toolchain caches.
    pub writable: Vec<String>,
    /// Home-relative paths that exist and an agent may read: git identity.
    pub readable: Vec<String>,
    /// The project's `.git`, when it has one.
    pub git: bool,
    pub runtime_dir: Option<String>,
    pub shell: String,
    /// The directory holding `tracelean-trace`, put first on the agent's
    /// `PATH` with the host's own `PATH` after it — so an agent gathers a
    /// requirement's context itself (`tracelean-trace . --context REQ-X`).
    pub tools: Option<(String, String)>,
    /// Where the agent skills are, as `TRACELEAN_SKILLS` in the sandbox: the
    /// project's `CLAUDE.md` (or its agent's equivalent) sends it there.
    pub skills: Option<String>,
}

/// What, under the home directory, an agent's shell needs writable.
///
/// Its own state first — without `.claude` it can neither log in nor write the
/// transcript the editor reads — then the caches a toolchain refills slowly.
pub const HOME_WRITABLE: &[&str] =
    &[".claude", ".claude.json", ".cargo", ".rustup", ".cache", ".npm", ".elan", ".local/share/claude", ".local/state/claude"];

/// What, under the home directory, an agent's shell may read.
pub const HOME_READABLE: &[&str] = &[".gitconfig", ".local/bin"];

/// The protected roots the sandbox shows the tool from the real tree,
/// read-only, rather than through the copy: `.git`, when the project has one.
/// `.tracelean` is copied instead (`workcopy::create`), since tools write there.
///
/// @implements REQ-OBS.workspace_is_a_copy
pub fn bound(host: &Host) -> Vec<String> {
    if host.git {
        vec![".git".to_string()]
    } else {
        Vec::new()
    }
}

/// The bubblewrap arguments for a session.
///
/// The host is read-only; the home directory is emptied and only what an agent
/// needs is put back; the copy is mounted at the project's own path, so the
/// real tree is not reachable from inside (`REQ-SBX.real_tree_untouched`); the
/// real `.git` is readable and not writable, because writes to a protected path
/// never come back (`REQ-SBX.protected_never_mirrored`) and a commit that
/// silently vanished would be worse than one refused.
///
/// @implements REQ-SBX.real_tree_untouched
pub fn launch_args(session: &Session, host: &Host) -> Vec<String> {
    let mut args: Vec<String> =
        ["bwrap", "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"].map(String::from).to_vec();
    let mut push = |flag: &str, from: &str, to: &str| {
        args.push(flag.to_string());
        args.push(from.to_string());
        args.push(to.to_string());
    };
    if let Some(home) = &host.home {
        // Emptied first: bind order matters, and everything put back below
        // lands on top of the empty directory.
        push("--tmpfs", home, "");
        for rel in &host.writable {
            let path = format!("{home}/{rel}");
            push("--bind", &path, &path);
        }
        for rel in &host.readable {
            let path = format!("{home}/{rel}");
            push("--ro-bind", &path, &path);
        }
    }
    push("--tmpfs", "/tmp", "");
    push("--bind", &session.work, &session.root);
    for name in bound(host) {
        let path = format!("{}/{name}", session.root);
        push("--ro-bind", &path, &path);
    }
    if let Some(runtime) = &host.runtime_dir {
        push("--bind", runtime, runtime);
    }
    if let Some((tools, path)) = &host.tools {
        push("--setenv", "PATH", &format!("{tools}:{path}"));
    }
    if let Some(skills) = &host.skills {
        push("--setenv", "TRACELEAN_SKILLS", skills);
    }
    // `--tmpfs` takes one path, not two: the empty third argument is dropped.
    args.retain(|arg| !arg.is_empty());
    args.extend(["--die-with-parent", "--chdir", &session.root, &host.shell].map(String::from));
    args
}

/// An argument as a POSIX shell reads it back unchanged.
fn quote(arg: &str) -> String {
    let plain = arg.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.=:@+,".contains(c));
    if plain && !arg.is_empty() {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// The one line the user pastes into their own terminal.
///
/// @implements ARCH-NO-DRIVING.user_runs_it
pub fn command_line(session: &Session, host: &Host) -> String {
    launch_args(session, host).iter().map(|arg| quote(arg)).collect::<Vec<_>>().join(" ")
}

/// Where a session's launcher is kept: beside its copy, never inside it, so
/// the agent cannot rewrite the box it runs in.
pub fn launcher_path(session: &Session) -> String {
    match session.work.strip_suffix("/work") {
        Some(dir) => format!("{dir}/enter.sh"),
        None => format!("{}.enter.sh", session.work),
    }
}

/// The launcher: the whole command, kept in a file so that what a person
/// copies and reads is one short line.
pub fn launcher(session: &Session, host: &Host) -> String {
    format!(
        "#!/bin/sh\n# TraceLean sandbox {}: the project is a copy, .git is read-only, home is hidden.\nexec {}\n",
        session.id,
        command_line(session, host)
    )
}

/// The line a person pastes: run the launcher.
///
/// @implements ARCH-NO-DRIVING.user_runs_it
pub fn short_command(session: &Session) -> String {
    format!("sh {}", quote(&launcher_path(session)))
}

/// The directory name Claude Code gives a project under `~/.claude/projects`:
/// its absolute path with every character that is not a letter or a digit
/// replaced by `-`.
///
/// The copy is mounted at the project's own path, so an agent inside the
/// sandbox writes its transcript under the project's name, not the copy's.
pub fn claude_slug(path: &str) -> String {
    path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            id: "s1".into(),
            root: "/home/u/proj".into(),
            work: "/home/u/proj/.tracelean/sessions/s1/work".into(),
        }
    }

    fn host() -> Host {
        Host {
            home: Some("/home/u".into()),
            writable: vec![".claude".into()],
            readable: vec![".gitconfig".into()],
            git: true,
            runtime_dir: None,
            shell: "/bin/bash".into(),
            tools: None,
            skills: None,
        }
    }

    /// The agent's shell finds TraceLean's own command and its skills, so it
    /// can gather what a change touches without being told where they are.
    #[test]
    fn the_agent_finds_the_trace_command_and_the_skills() {
        let mut with = host();
        with.tools = Some(("/opt/tl/bin".into(), "/usr/bin".into()));
        with.skills = Some("/opt/tl/skills".into());
        let joined = launch_args(&session(), &with).join(" ");
        assert!(joined.contains("--setenv PATH /opt/tl/bin:/usr/bin"), "{joined}");
        assert!(joined.contains("--setenv TRACELEAN_SKILLS /opt/tl/skills"), "{joined}");
        assert!(!launch_args(&session(), &host()).join(" ").contains("--setenv"));
    }

    /// The copy is where the project was, and the project is nowhere writable.
    ///
    /// @tests REQ-SBX.real_tree_untouched
    #[test]
    fn the_copy_is_mounted_over_the_project_and_git_is_read_only() {
        let args = launch_args(&session(), &host());
        let joined = args.join(" ");
        assert!(joined.contains("--bind /home/u/proj/.tracelean/sessions/s1/work /home/u/proj"));
        assert!(joined.contains("--ro-bind /home/u/proj/.git /home/u/proj/.git"));
        assert!(!joined.contains("--bind /home/u/proj /home/u/proj"), "the real tree is writable");
        // Home is emptied before anything is put back into it.
        let emptied = args.iter().position(|a| a == "/home/u").unwrap();
        let claude = args.iter().position(|a| a == "/home/u/.claude").unwrap();
        assert!(emptied < claude);
        assert_eq!(args.last().map(String::as_str), Some("/bin/bash"));
    }

    #[test]
    fn the_command_line_quotes_what_a_shell_would_split() {
        let mut odd = session();
        odd.root = "/home/u/my project's".into();
        let line = command_line(&odd, &host());
        assert!(line.contains("'/home/u/my project'\\''s'"), "{line}");
        assert!(line.starts_with("bwrap --ro-bind / /"));
    }

    #[test]
    fn the_launcher_sits_beside_the_copy_and_runs_the_whole_command() {
        assert_eq!(launcher_path(&session()), "/home/u/proj/.tracelean/sessions/s1/enter.sh");
        assert_eq!(short_command(&session()), "sh /home/u/proj/.tracelean/sessions/s1/enter.sh");
        let script = launcher(&session(), &host());
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains(&format!("exec {}", command_line(&session(), &host()))));
    }

    #[test]
    fn the_slug_is_the_one_claude_code_uses() {
        assert_eq!(claude_slug("/home/u/Desktop/trace_code_ide"), "-home-u-Desktop-trace-code-ide");
    }
}
