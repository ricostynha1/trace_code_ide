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
    /// Claude Code's own directory when `CLAUDE_CONFIG_DIR` moves it from
    /// `~/.claude`: put back writable, or the agent finds no login there.
    #[serde(default)]
    pub claude_config: Option<String>,
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
    // The host's login, wherever its Claude Code keeps it.
    if let Some(config) = &host.claude_config {
        push("--bind", config, config);
    }
    // What `PATH` and `TRACELEAN_SKILLS` name, read-only: usually under the
    // home just emptied. Before the copy, so a project that holds them (the
    // TraceLean tree itself) shows its own copy instead.
    if let Some((tools, _)) = &host.tools {
        push("--ro-bind", tools, tools);
    }
    if let Some(skills) = &host.skills {
        push("--ro-bind", skills, skills);
    }
    // The agent's brief, as a `CLAUDE.md` one directory above the project —
    // read by Claude Code for any project, traced or not — when that directory
    // is on the emptied home, so nothing real is covered or written.
    if let (Some(home), Some(parent)) = (&host.home, std::path::Path::new(&session.root).parent()) {
        if parent.starts_with(home) {
            let at = parent.join("CLAUDE.md").display().to_string();
            push("--ro-bind", &brief_path(session), &at);
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

/// Where a session's brief is kept: beside its launcher.
pub fn brief_path(session: &Session) -> String {
    match session.work.strip_suffix("/work") {
        Some(dir) => format!("{dir}/CLAUDE.md"),
        None => format!("{}.CLAUDE.md", session.work),
    }
}

/// What an agent started in the sandbox is told before anything else: that
/// the project is TraceLean's to check, and where the method and the tools
/// are — so a tree with no `CLAUDE.md` of its own still says how to work in it.
pub fn brief(host: &Host) -> String {
    let skills = host.skills.as_deref().unwrap_or("(not found: `skills/` in the TraceLean source tree)");
    format!(
        "# You are in a TraceLean sandbox\n\
         \n\
         The project below is a copy; a person reviews what you change before it\n\
         reaches the real tree. Everything you need is installed — install nothing.\n\
         \n\
         - `tracelean-trace` is on `PATH`. Start with `tracelean-trace .`: it says what\n\
         \x20 the project's requirements claim and what is missing, even when nothing\n\
         \x20 is traced yet.\n\
         - The method is in `$TRACELEAN_SKILLS` ({skills}). Read its `README.md`\n\
         \x20 before changing anything; `08-setup-in-case-of-error.md` if a command fails.\n\
         - Before changing what a requirement covers: `tracelean-trace . --context REQ-X.clause`.\n\
         - Before reporting: `tracelean-trace .` must end `blocking: false`, and\n\
         \x20 `tracelean-trace . --stale` must list only what is a person's.\n\
         \n\
         A `CLAUDE.md` in the project itself, if there is one, says more and wins.\n"
    )
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
            claude_config: None,
        }
    }

    /// An agent in any project, traced or not, is told where the method and
    /// the tools are: a brief one directory above the project, on the emptied
    /// home — and none when that directory is not on it.
    #[test]
    fn the_agent_is_briefed_above_the_project_on_the_emptied_home() {
        let args = launch_args(&session(), &host());
        let joined = args.join(" ");
        assert!(joined.contains("--ro-bind /home/u/proj/.tracelean/sessions/s1/CLAUDE.md /home/u/CLAUDE.md"), "{joined}");
        let mut deep = session();
        deep.root = "/home/u/code/proj".into();
        assert!(launch_args(&deep, &host()).join(" ").contains(" /home/u/code/CLAUDE.md"));
        let mut outside = session();
        outside.root = "/srv/proj".into();
        assert!(!launch_args(&outside, &host()).join(" ").contains("CLAUDE.md"));
        assert!(brief(&host()).contains("tracelean-trace ."));
    }

    /// A Claude Code configured elsewhere than `~/.claude` keeps its login
    /// there; the agent is logged in only if that directory is put back.
    #[test]
    fn a_moved_claude_config_keeps_the_hosts_login() {
        let mut with = host();
        with.claude_config = Some("/home/u/.claude-work".into());
        let args = launch_args(&session(), &with);
        let joined = args.join(" ");
        assert!(joined.contains("--bind /home/u/.claude-work /home/u/.claude-work"), "{joined}");
        let emptied = args.iter().position(|a| a == "/home/u").unwrap();
        let config = args.iter().position(|a| a == "/home/u/.claude-work").unwrap();
        assert!(emptied < config);
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

    /// Named is not reachable: home is emptied, and the checker and skills
    /// usually live under it, so both are put back, read-only — before the
    /// copy, which wins when the project holds them.
    #[test]
    fn what_the_agent_is_pointed_at_survives_the_emptied_home() {
        let mut with = host();
        with.tools = Some(("/home/u/tl/target/debug".into(), "/usr/bin".into()));
        with.skills = Some("/home/u/tl/skills".into());
        let args = launch_args(&session(), &with);
        let joined = args.join(" ");
        assert!(joined.contains("--ro-bind /home/u/tl/target/debug /home/u/tl/target/debug"), "{joined}");
        assert!(joined.contains("--ro-bind /home/u/tl/skills /home/u/tl/skills"), "{joined}");
        let emptied = args.iter().position(|a| a == "/home/u").unwrap();
        let skills = args.iter().position(|a| a == "/home/u/tl/skills").unwrap();
        let copy = args.iter().position(|a| a == "/home/u/proj/.tracelean/sessions/s1/work").unwrap();
        assert!(emptied < skills && skills < copy, "{joined}");
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
