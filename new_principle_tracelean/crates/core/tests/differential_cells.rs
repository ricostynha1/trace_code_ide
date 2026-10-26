mod harness;

use harness::{strukt, name};

use std::collections::BTreeMap;

use tracelean_core::drt::gen;
use tracelean_core::drt::schema::Schema;
use tracelean_core::surface::cells::{drawn_wrong, Look};
use tracelean_core::surface::view::Buffer;

fn role() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for nullary in ["plain", "added", "heading"] {
        variants.insert(nullary.to_string(), None);
    }
    variants.insert(
        "mark".to_string(),
        Some(Box::new(strukt(&[("mark", Schema::simple_enum(&["tested", "proved"])), ("held", Schema::Bool)]))),
    );
    Schema::Enum { variants }
}

const PIECES: &[&str] = &[
    "ab", "x", "+", "\n", "\r\n", "\r", "\t", "\u{1b}[2J", "\u{1b}[J", "\u{1b}[H", "\u{1b}[2;3H", "\u{1b}[;2H",
    "\u{1b}[0m", "\u{1b}[m", "\u{1b}[1m", "\u{1b}[2m", "\u{1b}[22m", "\u{1b}[7m", "\u{1b}[27m",
    "\u{1b}[0;1;38;2;0;255;0m", "\u{1b}[38;2;0;255;0m", "\u{1b}[0;38;2;255;0;0m", "\u{1b}[38;2;300;1;2m",
    "\u{1b}[38;2;1m", "\u{1b}[38;5;3;1m", "\u{1b}[48;2;1;2;3m", "\u{1b}[48;5m", "\u{1b}[39m", "\u{1b}[49m",
    "\u{1b}[38m", "\u{1b}(B", "\u{1b}=", "\u{1b}[?25l", "\u{1b}[99999999999999999999999H", "\u{7f}",
    "\u{1b}[?7l", "\u{1b}[?7h", "\u{1b}[0;9H", "abcdefgh", "\n\n\n\n",
];

fn painted() -> Vec<String> {
    let mut state: u64 = 410;
    let mut next = |below: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) % below
    };
    let mut out = vec![String::new()];
    for _ in 0..300 {
        let count = 1 + next(9);
        let mut s: String = (0..count).map(|_| PIECES[next(PIECES.len() as u64) as usize]).collect();
        if next(20) == 0 {
            s.push_str(if next(2) == 0 { "\u{1b}" } else { "\u{1b}[1" });
        }
        out.push(s);
    }
    out
}

fn buffer() -> Schema {
    let span = strukt(&[
        ("start", Schema::Nat { max: Some(4), edges: vec![0] }),
        ("stop", Schema::Nat { max: Some(6), edges: vec![0, 20] }),
        ("role", role()),
        ("actions", Schema::List { inner: Box::new(name(&["file.open"])), max_len: Some(0) }),
    ]);
    let mut kind = BTreeMap::new();
    kind.insert("file".to_string(), Some(Box::new(strukt(&[("path", name(&["a.rs"]))]))));
    strukt(&[
        ("id", name(&["file:a.rs"])),
        ("kind", Schema::Enum { variants: kind }),
        ("text", name(&["", "ab", "+a\nb", "abc\nde", "x\n\ny", "abcdefgh"])),
        ("spans", Schema::List { inner: Box::new(span), max_len: Some(3) }),
    ])
}

/// @drt REQ-LOOK.roles_drawn_in_theme_colours
/// @tests REQ-LOOK.roles_drawn_in_theme_colours
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_is_drawn_in_the_wrong_look() {
    let op = "REQ-LOOK.roles_drawn_in_theme_colours";
    let look = strukt(&[
        ("role", role()),
        ("fg", Schema::Option { inner: Box::new(name(&["#00ff00", "#ff0000"])) }),
        ("bold", Schema::Bool),
    ]);
    let painted: Vec<&str> = painted().leak().iter().map(String::as_str).collect();
    let schema = strukt(&[
        ("buffer", buffer()),
        ("painted", name(&painted)),
        ("rows", Schema::Nat { max: Some(4), edges: vec![0, 1] }),
        ("columns", Schema::Nat { max: Some(6), edges: vec![0, 1] }),
        ("looks", Schema::List { inner: Box::new(look), max_len: Some(3) }),
    ]);
    harness::agree(op, &schema, 410, 3_000);

    let mut rng = gen::Rng::new(410);
    let (mut right, mut wrong, mut off) = (0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let buffer: Buffer = serde_json::from_value(v["buffer"].clone()).unwrap();
        let looks: Vec<Look> = serde_json::from_value(v["looks"].clone()).unwrap();
        let (rows, columns) = (v["rows"].as_u64().unwrap(), v["columns"].as_u64().unwrap());
        let found = drawn_wrong(buffer.clone(), v["painted"].as_str().unwrap().to_string(), rows, columns, looks.clone());
        let coloured: Vec<_> = buffer
            .spans
            .iter()
            .filter(|s| s.start < s.stop && looks.iter().any(|l| l.role == s.role && l.fg.is_some()))
            .collect();
        if coloured.iter().any(|s| (s.start..s.stop).any(|o| o < buffer.text.chars().count() && !found.iter().any(|m| m.drawn.is_some() && offset_of(&buffer.text, m.line, m.column) == o))) {
            right += 1;
        }
        if found.iter().any(|m| m.expected.is_some() && m.drawn.is_some()) {
            wrong += 1;
        }
        if found.iter().any(|m| m.drawn.is_none()) {
            off += 1;
        }
    }
    harness::covered(
        op,
        &[
            ("a coloured role drawn in its look", right),
            ("a coloured role drawn otherwise", wrong),
            ("a character off the grid", off),
        ],
    );
}

fn offset_of(text: &str, line: u64, column: u64) -> usize {
    let (mut at_line, mut at_column) = (0, 0);
    for (offset, c) in text.chars().enumerate() {
        if (at_line, at_column) == (line, column) {
            return offset;
        }
        if c == '\n' {
            (at_line, at_column) = (at_line + 1, 0);
        } else {
            at_column += 1;
        }
    }
    usize::MAX
}
