//! Receipt rendering.
//!
//! A second language in the same project, for the same reason a real codebase
//! has one: this part is on a hot path and Python was not the right answer. The
//! traceability annotations are identical — the same roles, in a `//` comment —
//! because the scanner works on tree-sitter comment nodes, not on file
//! extensions.
//!
//! Worth knowing: writing an annotation's literal spelling in prose makes a
//! real annotation. The checker reports it as an unknown role or a qualifier
//! with nothing to qualify, which is the correct reaction — it cannot tell
//! "I am documenting this syntax" from "I meant it". Describe roles by name in
//! prose, as this paragraph does.

/// One line of the customer's cart.
#[derive(Debug, Clone)]
pub struct CartLine {
    pub description: String,
    pub cents: u64,
}

/// A rendered receipt, ready to show or print.
#[derive(Debug, Clone)]
pub struct Receipt {
    pub lines: Vec<String>,
    pub total_cents: u64,
}

// @implements REQ-RECEIPT.lines
pub fn render(lines: &[CartLine], total_cents: u64) -> Receipt {
    Receipt {
        lines: lines
            .iter()
            .map(|line| format!("{:<28} {:>8}", line.description, format_cents(line.cents)))
            .collect(),
        total_cents,
    }
}

// @exempt REQ-RECEIPT.audit reason="append-only side effect; the Lean model is pure" by=ricardo
//
// An exemption is a decision with a name on it, not a gap. The clause stays in
// the denominator — the requirement can never read as 100% because of it — and
// the reason is what a reviewer argues with six months from now.
pub fn append_to_audit_log(receipt: &Receipt) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("audit.log")?;
    for line in &receipt.lines {
        writeln!(file, "{}", line)?;
    }
    writeln!(file, "TOTAL {}", format_cents(receipt.total_cents))
}

fn format_cents(cents: u64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}
