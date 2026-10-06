//! Checking a frontend against the representation it was given.
//!
//! A frontend is a process: it reads a buffer and answers with what it drew —
//! the lines it put on screen and the actions it offered, with where. That
//! answer is a value, so this is the same shape as every other check in the
//! system, and the same code checks a terminal, a web view, or anything else
//! somebody writes.
//!
//! What is not checked is how it looks. That is the frontend's business, and a
//! person's to judge.
//!
//! @implements REQ-VIEW.frontend_is_checkable

use serde::{Deserialize, Serialize};

use crate::drt::protocol::RunnerError;
use crate::drt::run::{Runner, RunnerSpec};
use crate::surface::view::{conformance, Breach, Buffer, Rendering};

/// The op a frontend answers.
pub const RENDER: &str = "render";

/// What one buffer's answer amounted to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Verdict {
    /// It drew the buffer.
    Conformant,
    /// It drew something else, or offered something nobody declared.
    Breached { breaches: Vec<Breach> },
    /// It refused the buffer. `rendering_is_total` says it may not: a kind a
    /// frontend cannot draw richly is still a kind it must draw as text.
    Refused { message: String },
    /// It answered with something that is not a rendering at all.
    Unreadable { message: String },
}

/// One buffer, and what the frontend did with it.
#[derive(Debug, Clone)]
pub struct Checked {
    pub case: u64,
    pub buffer: Buffer,
    pub verdict: Verdict,
}

/// What a whole run of buffers established.
#[derive(Debug, Clone)]
pub struct Report {
    pub checked: Vec<Checked>,
}

impl Report {
    /// Whether every buffer was drawn as the buffer it was.
    pub fn conformant(&self) -> bool {
        self.checked.iter().all(|c| c.verdict == Verdict::Conformant)
    }

    /// The first buffer it got wrong, which is the one to look at.
    pub fn first_failure(&self) -> Option<&Checked> {
        self.checked.iter().find(|c| c.verdict != Verdict::Conformant)
    }
}

/// Judge one answer. Pure: the process talking is the caller's problem.
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

/// Show a frontend every buffer in turn and check what it says it drew.
///
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

/// What a screen-reading harness sends and reads.
///
/// The check above asks a frontend what it drew and believes the answer. That
/// is the frontend agreeing with itself: a program whose painting and whose
/// reporting are two code paths passes it while showing something else
/// entirely. Reading the screen closes that gap — the frontend writes what a
/// terminal would receive and says nothing about it, so the text checked is the
/// text a person would see.
///
/// What a screen cannot carry is what is *offered*: an action is visible only
/// where a frontend chooses to show it. So this checks the text and leaves the
/// affordances to `check`, and the two together are worth more than either.
///
/// @implements REQ-VIEW.screen_is_readable
/// @implements REQ-VIEW.text_is_the_content
pub struct Screen {
    /// Written between screens, so one painted screen can be told from the next.
    pub separator: String,
}

impl Default for Screen {
    fn default() -> Self {
        // A NUL: no line of text contains one, and no terminal prints one.
        Screen { separator: "\u{0}".to_string() }
    }
}

/// What was left on the screen once the escape sequences are taken out: the
/// lines a person would read, or `None` if nothing was painted at all.
///
/// Only the escape forms a terminal is actually sent — CSI sequences and the
/// two-character ones — because a full terminal emulator is a dependency, not a
/// check, and a frontend whose screen needs one to be understood has already
/// stopped being checkable.
///
/// Nothing painted and one empty line painted are different: the first is an
/// empty byte stream, the second is a single newline. Keeping them apart is why
/// this answers with an `Option` rather than an empty list — a frontend that
/// refuses a buffer and one that draws an empty file must not look alike.
///
/// Public because a driven frontend is read the same way (`REQ-DRIVE`): the
/// suite that presses keys at a real terminal must arrive at the text a person
/// would have read, and a second reader written for it could disagree with this
/// one about what was on the screen.
///
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
            // CSI: `ESC [ … <final byte>`, however long the parameters are.
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            // A character-set designator: `ESC ( B` and its relatives, which
            // carry an intermediate byte before the one that ends them.
            Some('(') | Some(')') | Some('*') | Some('+') => {
                chars.next();
                chars.next();
            }
            // Everything else is `ESC` and one byte.
            _ => {
                chars.next();
            }
        }
    }
    if out.is_empty() {
        return None;
    }
    // Every painted line ends in a newline, so the last one leaves a trailing
    // empty piece that was never a line.
    let trimmed = out.strip_suffix('\n').unwrap_or(&out);
    Some(trimmed.split('\n').map(str::to_string).collect())
}

/// Show a frontend every buffer in turn and read the screen it painted.
///
/// The verdict is about the text alone: a rendering built from what was painted
/// carries no offered actions, so `conformance` would report every declared
/// action as dropped. What is checked here is that the screen is the buffer's
/// text — and that a frontend which paints nothing has refused a buffer it was
/// required to draw.
///
/// @implements REQ-VIEW.screen_is_readable
/// @implements REQ-VIEW.rendering_is_total
pub fn capture(
    spec: &RunnerSpec,
    screen: &Screen,
    buffers: Vec<Buffer>,
) -> Result<Report, RunnerError> {
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

    let mut checked = Vec::new();
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
        let text = String::from_utf8_lossy(&painted).replace(&screen.separator, "");
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
        checked.push(Checked { case, buffer, verdict });
    }
    let _ = child.kill();
    Ok(Report { checked })
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

        // A frontend that refuses a buffer has failed `rendering_is_total`.
        assert!(matches!(
            verdict_of(&buffer, None, Some("cannot draw a listing".into())),
            Verdict::Refused { .. }
        ));
        // And one that answers with something else has failed to answer at all.
        assert!(matches!(
            verdict_of(&buffer, Some(serde_json::json!({"pixels": 3})), None),
            Verdict::Unreadable { .. }
        ));
        assert!(matches!(verdict_of(&buffer, None, None), Verdict::Unreadable { .. }));
    }

    /// Reading a screen: the escapes go, the text stays, and an empty line is
    /// not the same as an empty screen.
    ///
    /// @tests REQ-VIEW.text_is_the_content
    #[test]
    fn a_screen_is_read_as_the_text_a_person_would_see() {
        assert_eq!(
            visible("\u{1b}[2Jone\ntwo\n\u{1b}[0m"),
            Some(vec!["one".to_string(), "two".to_string()])
        );
        // A trailing blank line is content; the newline ending the last line is not.
        assert_eq!(
            visible("\u{1b}[2Jone\n\n\u{1b}[0m"),
            Some(vec!["one".to_string(), String::new()])
        );
        // One empty line.
        assert_eq!(visible("\u{1b}[2J\n\u{1b}[0m"), Some(vec![String::new()]));
        // Nothing at all, which is a refusal rather than an empty buffer.
        assert_eq!(visible("\u{1b}[2J\u{1b}[0m"), None);
        // Colour, cursor moves and a two-character escape are all invisible.
        assert_eq!(
            visible("\u{1b}[1;1H\u{1b}[31mred\u{1b}(B\n"),
            Some(vec!["red".to_string()])
        );
    }
}
