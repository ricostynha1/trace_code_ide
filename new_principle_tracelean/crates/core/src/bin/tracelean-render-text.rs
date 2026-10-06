//! A frontend, as small as one can be: it draws a buffer as text.
//!
//! This exists to be checked. It reads one buffer a line and answers with what
//! it drew, which is what `REQ-VIEW.frontend_is_checkable` asks of any frontend
//! — a terminal, a web view, or this.
//!
//! The flags make it draw *wrongly* on purpose, so the check can be shown to
//! have teeth. A conformance suite that only ever sees a correct frontend
//! establishes nothing: it would pass against a checker that always said yes.
//!
//! ```text
//! tracelean-render-text              # draws the buffer
//! tracelean-render-text --paint      # paints the screen, as a terminal receives it
//! tracelean-render-text --embroider  # decorates the text it was given
//! tracelean-render-text --invent     # offers an action nobody declared
//! tracelean-render-text --drop       # keeps an action to itself
//! tracelean-render-text --refuse     # will not draw a menu
//! tracelean-render-text --two-faced  # reports faithfully, paints something else
//! ```
//!
//! `--paint` is the difference between a frontend that *says* what it drew and
//! one that is read by what it actually put on the screen. In paint mode the
//! answer is not a report: the process writes the screen, the harness reads the
//! bytes a terminal would have received, and the frontend has no opportunity to
//! describe itself. A frontend whose painting and whose reporting are two code
//! paths can pass the first check and fail this one — which is what
//! `--two-faced` is: it answers with the buffer's own text and puts something
//! else on the screen. Not a frontend anybody would write on purpose; exactly
//! what a frontend becomes when its drawing is changed and its reporting is
//! not.
//!
//! @implements REQ-VIEW.frontend_is_checkable

use std::io::{BufRead, Write};

use tracelean_core::surface::view::{actions_at, plain_text, Buffer, BufferKind, Rendering};

/// How this frontend misbehaves, if it does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    Embroider,
    Invent,
    Drop,
    Refuse,
}

/// Where a frontend offers actions: one position per span, which is where the
/// thing the span is about starts.
fn offer_positions(buffer: &Buffer) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for span in &buffer.spans {
        if span.start < span.stop && !out.contains(&span.start) {
            out.push(span.start);
        }
    }
    out
}

fn render(buffer: &Buffer, fault: Fault) -> Rendering {
    let mut lines = plain_text(buffer.clone());
    if fault == Fault::Embroider {
        // The kind of thing a frontend does when it decides it knows better.
        if let Some(first) = lines.first_mut() {
            *first = format!("📁 {first}");
        }
    }

    let mut offered: Vec<(usize, String)> = Vec::new();
    for offset in offer_positions(buffer) {
        for action in actions_at(buffer.clone(), offset) {
            offered.push((offset, action));
        }
    }
    match fault {
        Fault::Invent => offered.push((0, "file.delete".to_string())),
        Fault::Drop => {
            offered.pop();
        }
        _ => {}
    }

    Rendering { lines, offered }
}

/// The bytes a terminal receives for one screen: the lines, each ended by a
/// newline, wrapped in the markers a screen-reading harness uses to tell one
/// painted screen from the next.
///
/// Real terminals delimit with cursor moves and clears; this is the same idea
/// with nothing to parse, because what is being checked is the text on the
/// screen rather than the escape sequences that put it there.
fn paint(lines: &[String]) -> String {
    let mut out = String::from("\u{1b}[2J");
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("\u{1b}[0m\u{0}");
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let painting = args.iter().any(|a| a == "--paint");
    // Two-faced: the fault applies to the screen only, never to the report.
    let two_faced = args.iter().any(|a| a == "--two-faced");
    let fault = if args.iter().any(|a| a == "--embroider") {
        Fault::Embroider
    } else if args.iter().any(|a| a == "--invent") {
        Fault::Invent
    } else if args.iter().any(|a| a == "--drop") {
        Fault::Drop
    } else if args.iter().any(|a| a == "--refuse") {
        Fault::Refuse
    } else {
        Fault::None
    };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let asked: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(e) => {
                let _ = writeln!(stdout, "{}", serde_json::json!({"case": 0, "error": e.to_string()}));
                let _ = stdout.flush();
                continue;
            }
        };
        let case = asked["case"].as_u64().unwrap_or(0);
        let parsed = serde_json::from_value::<Buffer>(asked["input"].clone());
        let refusing = parsed
            .as_ref()
            .map(|b| fault == Fault::Refuse && matches!(b.kind, BufferKind::Menu { .. }))
            .unwrap_or(false);

        if painting {
            // Paint mode: the screen goes out as a terminal would receive it,
            // and nothing describes it. A refusal paints nothing, which is what
            // a frontend that will not draw a kind looks like from outside.
            let painted_fault = if two_faced { Fault::Embroider } else { fault };
            let screen = match (&parsed, refusing) {
                (Ok(buffer), false) => paint(&render(buffer, painted_fault).lines),
                _ => paint(&[]),
            };
            let _ = write!(stdout, "{screen}");
            let _ = stdout.flush();
            continue;
        }

        let reply = match parsed {
            Err(e) => serde_json::json!({"case": case, "error": e.to_string()}),
            Ok(buffer) => {
                if refusing {
                    serde_json::json!({"case": case, "error": "this frontend does not draw menus"})
                } else {
                    let drawn = render(&buffer, fault);
                    serde_json::json!({"case": case, "output": drawn})
                }
            }
        };
        let _ = writeln!(stdout, "{reply}");
        let _ = stdout.flush();
    }
}
