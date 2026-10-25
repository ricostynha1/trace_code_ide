//! @implements REQ-VIEW.frontend_is_checkable

use serde::{Deserialize, Serialize};

use crate::drt::protocol::RunnerError;
use crate::drt::run::{Runner, RunnerSpec};
use crate::surface::view::{conformance, Breach, Buffer, Rendering};

pub const RENDER: &str = "render";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Verdict {
    Conformant,
    Breached { breaches: Vec<Breach> },
    Refused { message: String },
    Unreadable { message: String },
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub case: u64,
    pub buffer: Buffer,
    pub verdict: Verdict,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub checked: Vec<Checked>,
}

impl Report {
    pub fn conformant(&self) -> bool {
        self.checked.iter().all(|c| c.verdict == Verdict::Conformant)
    }

    pub fn first_failure(&self) -> Option<&Checked> {
        self.checked.iter().find(|c| c.verdict != Verdict::Conformant)
    }
}

fn verdict_of(buffer: &Buffer, output: Option<serde_json::Value>, error: Option<String>) -> Verdict {
    if let Some(message) = error {
        return Verdict::Refused { message };
    }
    let Some(value) = output else {
        return Verdict::Unreadable { message: "neither a rendering nor an error".to_string() };
    };
    match serde_json::from_value::<Rendering>(value) {
        Err(e) => Verdict::Unreadable { message: e.to_string() },
        Ok(rendering) => match conformance(buffer.clone(), rendering) {
            breaches if breaches.is_empty() => Verdict::Conformant,
            breaches => Verdict::Breached { breaches },
        },
    }
}

/// @implements REQ-VIEW.frontend_is_checkable
/// @implements REQ-VIEW.rendering_is_total
pub fn check(spec: &RunnerSpec, buffers: Vec<Buffer>) -> Result<Report, RunnerError> {
    let mut runner = Runner::start(spec)?;
    let mut checked = Vec::new();
    for (index, buffer) in buffers.into_iter().enumerate() {
        let case = index as u64;
        let reply = runner.ask(case, RENDER, &serde_json::to_value(&buffer).expect("a buffer"))?;
        let verdict = verdict_of(&buffer, reply.output, reply.error);
        checked.push(Checked { case, buffer, verdict });
    }
    Ok(Report { checked })
}

/// @implements REQ-VIEW.screen_is_readable
/// @implements REQ-VIEW.text_is_the_content
pub struct Screen {
    pub separator: String,
}

impl Default for Screen {
    fn default() -> Self {
        Screen { separator: "\u{0}".to_string() }
    }
}

/// @implements REQ-VIEW.screen_is_readable
/// @implements REQ-DRIVE.screen_answers_for_the_frontend
pub fn readable(painted: &str) -> Option<Vec<String>> {
    visible(painted)
}

fn visible(painted: &str) -> Option<Vec<String>> {
    let mut out = String::new();
    let mut chars = painted.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some('(') | Some(')') | Some('*') | Some('+') => {
                chars.next();
                chars.next();
            }
            _ => {
                chars.next();
            }
        }
    }
    if out.is_empty() {
        return None;
    }
    let trimmed = out.strip_suffix('\n').unwrap_or(&out);
    Some(trimmed.split('\n').map(str::to_string).collect())
}

/// @implements REQ-VIEW.screen_is_readable
/// @implements REQ-VIEW.rendering_is_total
pub fn capture(
    spec: &RunnerSpec,
    screen: &Screen,
    buffers: Vec<Buffer>,
) -> Result<Report, RunnerError> {
    let screens = painted_screens(spec, screen, buffers.clone())?;
    let mut checked = Vec::new();
    for (index, (buffer, text)) in buffers.into_iter().zip(screens).enumerate() {
        let verdict = match visible(&text) {
            None => Verdict::Refused { message: "painted nothing".to_string() },
            Some(lines) => {
                let rendering = Rendering { lines, offered: Vec::new() };
                match conformance(buffer.clone(), rendering)
                    .into_iter()
                    .filter(|b| !matches!(b, Breach::ActionDropped { .. }))
                    .collect::<Vec<_>>()
                {
                    breaches if breaches.is_empty() => Verdict::Conformant,
                    breaches => Verdict::Breached { breaches },
                }
            }
        };
        checked.push(Checked { case: index as u64, buffer, verdict });
    }
    Ok(Report { checked })
}

pub fn painted_screens(spec: &RunnerSpec, screen: &Screen, buffers: Vec<Buffer>) -> Result<Vec<String>, RunnerError> {
    use std::io::{BufRead, Write};

    let (program, args) = spec
        .cmd
        .split_first()
        .ok_or_else(|| RunnerError::Spawn("no command given".to_string()))?;
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    let mut child = command.spawn().map_err(|e| RunnerError::Spawn(e.to_string()))?;
    let mut stdin = child.stdin.take().expect("piped");
    let mut stdout = std::io::BufReader::new(child.stdout.take().expect("piped"));

    let mut screens = Vec::new();
    for (index, buffer) in buffers.into_iter().enumerate() {
        let case = index as u64;
        let asked = serde_json::json!({
            "case": case,
            "op": RENDER,
            "input": serde_json::to_value(&buffer).expect("a buffer"),
        });
        writeln!(stdin, "{asked}").map_err(|e| RunnerError::Died(e.to_string()))?;
        stdin.flush().map_err(|e| RunnerError::Died(e.to_string()))?;

        let mut painted = Vec::new();
        let separator = screen.separator.as_bytes()[0];
        match stdout.read_until(separator, &mut painted) {
            Ok(0) => return Err(RunnerError::Died("closed its pipe".to_string())),
            Ok(_) => {}
            Err(e) => return Err(RunnerError::Died(e.to_string())),
        }
        screens.push(String::from_utf8_lossy(&painted).replace(&screen.separator, ""));
    }
    let _ = child.kill();
    Ok(screens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{directory_buffer, plain_text};

    fn listing() -> Buffer {
        directory_buffer("src".into(), vec![(0, "main.rs".into()), (0, "lib.rs".into())])
    }

    /// @tests REQ-VIEW.frontend_is_checkable
    #[test]
    fn an_answer_is_judged_without_any_process_running() {
        let buffer = listing();
        let drawn = Rendering {
            lines: plain_text(buffer.clone()),
            offered: vec![(0, "file.open".into()), (8, "file.open".into())],
        };
        let output = serde_json::to_value(&drawn).unwrap();
        assert_eq!(verdict_of(&buffer, Some(output), None), Verdict::Conformant);

        assert!(matches!(
            verdict_of(&buffer, None, Some("cannot draw a listing".into())),
            Verdict::Refused { .. }
        ));
        assert!(matches!(
            verdict_of(&buffer, Some(serde_json::json!({"pixels": 3})), None),
            Verdict::Unreadable { .. }
        ));
        assert!(matches!(verdict_of(&buffer, None, None), Verdict::Unreadable { .. }));
    }

    /// @tests REQ-VIEW.text_is_the_content
    #[test]
    fn a_screen_is_read_as_the_text_a_person_would_see() {
        assert_eq!(
            visible("\u{1b}[2Jone\ntwo\n\u{1b}[0m"),
            Some(vec!["one".to_string(), "two".to_string()])
        );
        assert_eq!(
            visible("\u{1b}[2Jone\n\n\u{1b}[0m"),
            Some(vec!["one".to_string(), String::new()])
        );
        assert_eq!(visible("\u{1b}[2J\n\u{1b}[0m"), Some(vec![String::new()]));
        assert_eq!(visible("\u{1b}[2J\u{1b}[0m"), None);
        assert_eq!(
            visible("\u{1b}[1;1H\u{1b}[31mred\u{1b}(B\n"),
            Some(vec!["red".to_string()])
        );
    }
}
