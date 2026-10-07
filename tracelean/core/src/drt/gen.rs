//! Deterministic case generation and shrinking.
//!
//! Determinism is a hard requirement, not a convenience: an evidence record
//! claims "12,000,000 cases, seed 7, zero divergences", and that claim is only
//! checkable if the same seed reproduces the same cases exactly. Hence a small
//! inline PRNG rather than a library whose stream can change between versions,
//! and hand-written shrinking rather than a typed shrinker whose regression
//! files would fight the seed.

use serde_json::{json, Map, Value};

use super::schema::Schema;

/// xorshift64*, chosen because it is short enough to read and its stream is
/// fixed here forever.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // Zero is a fixed point of xorshift; nudge it.
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next_u64() % n
        }
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            items.get(self.below(items.len() as u64) as usize)
        }
    }
}

/// Values worth trying far more often than chance would suggest — the places
/// where implementations and models actually disagree.
const NAT_EDGES: &[u64] = &[0, 1, 2, 7, 8, 9, 255, 256, 1023, 65535, u32::MAX as u64];
const INT_EDGES: &[i64] = &[0, 1, -1, 2, -2, i32::MIN as i64, i32::MAX as i64];
const STR_EDGES: &[&str] = &[
    "",
    " ",
    "a",
    "hunter2",
    "hunter22",
    "ünïcödé",
    "\u{1F600}",
    "line\nbreak",
    "quote\"inside",
    "very-long-value-used-to-probe-length-limits-0123456789",
];

/// Generate one value for a schema.
pub fn generate(schema: &Schema, rng: &mut Rng) -> Value {
    match schema {
        Schema::Nat { max, edges } => {
            let cap = max.unwrap_or(u32::MAX as u64);
            // Edge values dominate: uniform sampling almost never produces the
            // boundary that a `>= 8` check turns on.
            if rng.chance(60) {
                let candidates: Vec<u64> = NAT_EDGES
                    .iter()
                    .chain(edges.iter())
                    .copied()
                    .filter(|v| *v <= cap)
                    .collect();
                json!(rng.pick(&candidates).copied().unwrap_or(0))
            } else {
                json!(rng.below(cap.saturating_add(1)))
            }
        }
        Schema::Int { min, max } => {
            let lo = min.unwrap_or(i32::MIN as i64);
            let hi = max.unwrap_or(i32::MAX as i64);
            if rng.chance(60) {
                let candidates: Vec<i64> =
                    INT_EDGES.iter().copied().filter(|v| *v >= lo && *v <= hi).collect();
                json!(rng.pick(&candidates).copied().unwrap_or(lo))
            } else {
                let span = (hi as i128 - lo as i128).max(0) as u64;
                json!(lo + rng.below(span.saturating_add(1)) as i64)
            }
        }
        Schema::Bool => json!(rng.chance(50)),
        Schema::Str { max_len, examples } => {
            let cap = max_len.unwrap_or(64);
            if !examples.is_empty() && rng.chance(40) {
                return json!(rng.pick(examples).cloned().unwrap_or_default());
            }
            if rng.chance(50) {
                let candidates: Vec<&&str> =
                    STR_EDGES.iter().filter(|s| s.len() <= cap).collect();
                return json!(rng.pick(&candidates).map(|s| s.to_string()).unwrap_or_default());
            }
            let len = rng.below(cap as u64 + 1) as usize;
            let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789 _-".chars().collect();
            let s: String = (0..len)
                .map(|_| *rng.pick(&alphabet).unwrap_or(&'a'))
                .collect();
            json!(s)
        }
        Schema::Option { inner } => {
            // `none` far more often than chance: it is the case implementations
            // forget.
            if rng.chance(30) {
                Value::Null
            } else {
                generate(inner, rng)
            }
        }
        Schema::List { inner, max_len } => {
            let cap = max_len.unwrap_or(8);
            let len = if rng.chance(25) { 0 } else { rng.below(cap as u64 + 1) as usize };
            Value::Array((0..len).map(|_| generate(inner, rng)).collect())
        }
        Schema::Struct { fields } => {
            let mut map = Map::new();
            for (name, field) in fields {
                map.insert(name.clone(), generate(field, rng));
            }
            Value::Object(map)
        }
        Schema::Enum { variants } => {
            let names: Vec<&String> = variants.keys().collect();
            let Some(name) = rng.pick(&names).copied() else { return Value::Null };
            match variants.get(name).and_then(|p| p.as_ref()) {
                // Lean derives a nullary constructor as the bare string.
                None => json!(name),
                Some(payload) => {
                    let mut map = Map::new();
                    map.insert(name.clone(), generate(payload, rng));
                    Value::Object(map)
                }
            }
        }
    }
}

/// Structurally smaller candidates, tried in order until one still diverges.
///
/// Bounded and strictly reducing, so shrinking always terminates — an
/// unbounded shrinker on a flaky divergence would run forever.
pub fn shrink(value: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    match value {
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                for candidate in [0u64, 1, u / 2] {
                    if candidate < u {
                        out.push(json!(candidate));
                    }
                }
            } else if let Some(i) = n.as_i64() {
                for candidate in [0i64, i / 2] {
                    if candidate.abs() < i.abs() {
                        out.push(json!(candidate));
                    }
                }
            }
        }
        Value::String(s) => {
            if !s.is_empty() {
                out.push(json!(""));
                let half: String = s.chars().take(s.chars().count() / 2).collect();
                if half.len() < s.len() {
                    out.push(json!(half));
                }
                // Strip non-ascii before shortening further: a unicode-only
                // difference is worth isolating from a length difference.
                let ascii: String = s.chars().filter(|c| c.is_ascii_graphic()).collect();
                if ascii != *s {
                    out.push(json!(ascii));
                }
            }
        }
        Value::Bool(true) => out.push(json!(false)),
        Value::Array(items) => {
            if !items.is_empty() {
                out.push(json!([]));
                if items.len() > 1 {
                    out.push(Value::Array(items[..items.len() / 2].to_vec()));
                    out.push(Value::Array(items[1..].to_vec()));
                }
                for (i, item) in items.iter().enumerate() {
                    for smaller in shrink(item) {
                        let mut copy = items.clone();
                        copy[i] = smaller;
                        out.push(Value::Array(copy));
                    }
                }
            }
        }
        Value::Object(map) => {
            for (key, field) in map {
                for smaller in shrink(field) {
                    let mut copy = map.clone();
                    copy.insert(key.clone(), smaller);
                    out.push(Value::Object(copy));
                }
            }
        }
        _ => {}
    }
    out
}
