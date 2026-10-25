//! @implements REQ-VIEW.frontend_is_checkable

use std::io::{BufRead, Write};

use tracelean_core::surface::view::{actions_at, plain_text, Buffer, BufferKind, Rendering};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    Embroider,
    Invent,
    Drop,
    Refuse,
}

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
