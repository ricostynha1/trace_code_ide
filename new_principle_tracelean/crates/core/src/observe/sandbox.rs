use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub root: String,
    pub work: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub home: Option<String>,
    pub writable: Vec<String>,
    pub readable: Vec<String>,
    pub git: bool,
    pub runtime_dir: Option<String>,
    pub shell: String,
    pub tools: Option<(String, String)>,
    pub skills: Option<String>,
    #[serde(default)]
    pub claude_config: Option<String>,
}

pub const HOME_WRITABLE: &[&str] =
    &[".claude", ".claude.json", ".cargo", ".rustup", ".cache", ".npm", ".elan", ".local/share/claude", ".local/state/claude"];

pub const HOME_READABLE: &[&str] = &[".gitconfig", ".local/bin"];

/// @implements REQ-OBS.workspace_is_a_copy
pub fn bound(host: &Host) -> Vec<String> {
    if host.git {
        vec![".git".to_string()]
    } else {
        Vec::new()
    }
}

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
    if let Some(config) = &host.claude_config {
        push("--bind", config, config);
    }
    if let Some((tools, _)) = &host.tools {
        push("--ro-bind", tools, tools);
    }
    if let Some(skills) = &host.skills {
        push("--ro-bind", skills, skills);
    }
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
    args.retain(|arg| !arg.is_empty());
    args.extend(["--die-with-parent", "--chdir", &session.root, &host.shell].map(String::from));
    args
}

fn quote(arg: &str) -> String {
    let plain = arg.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.=:@+,".contains(c));
    if plain && !arg.is_empty() {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// @implements ARCH-NO-DRIVING.user_runs_it
pub fn command_line(session: &Session, host: &Host) -> String {
    launch_args(session, host).iter().map(|arg| quote(arg)).collect::<Vec<_>>().join(" ")
}

pub fn launcher_path(session: &Session) -> String {
    match session.work.strip_suffix("/work") {
        Some(dir) => format!("{dir}/enter.sh"),
        None => format!("{}.enter.sh", session.work),
    }
}

pub fn brief_path(session: &Session) -> String {
    match session.work.strip_suffix("/work") {
        Some(dir) => format!("{dir}/CLAUDE.md"),
        None => format!("{}.CLAUDE.md", session.work),
    }
}

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
         - `tracelean-view . <view>` prints what the person sees in the editor:\n\
         \x20 `requirements`, `design`, `requirement REQ-X`, `trace FILE`, `findings`, …\n\
         \x20 (`tracelean-view` alone lists them).\n\
         - Before changing what a requirement covers: `tracelean-trace . --context REQ-X.clause`.\n\
         - Before reporting: `tracelean-trace .` must end `blocking: false`, and\n\
         \x20 `tracelean-trace . --stale` must list only what is a person's.\n\
         \n\
         A `CLAUDE.md` in the project itself, if there is one, says more and wins.\n"
    )
}

pub fn launcher(session: &Session, host: &Host) -> String {
    format!(
        "#!/bin/sh\n# TraceLean sandbox {}: the project is a copy, .git is read-only, home is hidden.\nexec {}\n",
        session.id,
        command_line(session, host)
    )
}

/// @implements ARCH-NO-DRIVING.user_runs_it
pub fn short_command(session: &Session) -> String {
    format!("sh {}", quote(&launcher_path(session)))
}

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

    /// @tests REQ-SBX.real_tree_untouched
    #[test]
    fn the_copy_is_mounted_over_the_project_and_git_is_read_only() {
        let args = launch_args(&session(), &host());
        let joined = args.join(" ");
        assert!(joined.contains("--bind /home/u/proj/.tracelean/sessions/s1/work /home/u/proj"));
        assert!(joined.contains("--ro-bind /home/u/proj/.git /home/u/proj/.git"));
        assert!(!joined.contains("--bind /home/u/proj /home/u/proj"), "the real tree is writable");
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
