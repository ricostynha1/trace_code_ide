use serde_json::{Map, Value};

use super::schema::Schema;

/// @implements REQ-DRT-GEN.fixed_stream
/// @implements ARCH-DETERMINISM.seeded_generation
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
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
        if n == 0 { 0 } else { self.next_u64() % n }
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// @implements REQ-DRT-GEN.fixed_stream
/// @implements ARCH-DETERMINISM.seeded_generation
/// @drt REQ-DRT-GEN.fixed_stream
/// @drt ARCH-DETERMINISM.seeded_generation
pub fn raw_stream(seed: u64, count: u64) -> Vec<u64> {
    let mut rng = Rng::new(seed);
    (0..count).map(|_| rng.next_u64()).collect()
}

/// @implements REQ-DRT-GEN.edges_sampled
/// @drt REQ-DRT-GEN.edges_sampled
pub fn nat_stream(seed: u64, max: Option<u64>, edges: Vec<u64>, count: u64) -> Vec<u64> {
    let schema = Schema::Nat { max, edges };
    let mut rng = Rng::new(seed);
    (0..count).map(|_| value(&schema, &mut rng).as_u64().unwrap_or(0)).collect()
}

pub fn value_stream(seed: u64, schema: Schema, count: u64) -> Vec<Value> {
    let mut rng = Rng::new(seed);
    (0..count).map(|_| value(&schema, &mut rng)).collect()
}

pub(crate) const SHAPES: &str ="[{\"type\":\"nat\",\"max\":50,\"edges\":[7]},{\"type\":\"int\",\"min\":-5,\"max\":5},{\"type\":\"bool\"},{\"type\":\"str\",\"max_len\":4,\"examples\":[\"one\",\"\"]},{\"type\":\"option\",\"inner\":{\"type\":\"nat\",\"max\":9}},{\"type\":\"list\",\"inner\":{\"type\":\"int\",\"min\":0,\"max\":3},\"max_len\":3},{\"type\":\"tuple\",\"items\":[{\"type\":\"bool\"},{\"type\":\"nat\",\"max\":3}]},{\"type\":\"struct\",\"fields\":{\"a\":{\"type\":\"bool\"},\"b\":{\"type\":\"str\",\"max_len\":2}}},{\"type\":\"enum\",\"variants\":{\"x\":null,\"y\":{\"type\":\"nat\",\"max\":2}}},{\"type\":\"list\",\"inner\":{\"type\":\"struct\",\"fields\":{\"n\":{\"type\":\"option\",\"inner\":{\"type\":\"int\",\"min\":-1,\"max\":1}}}},\"max_len\":2}]";

/// @implements REQ-DRT-GEN.seed_reproduces
/// @drt REQ-DRT-GEN.seed_reproduces
pub fn shape_stream(seed: u64, shape: u64, count: u64) -> Vec<Value> {
    match shape_at(shape) {
        Some(schema) => value_stream(seed, schema, count),
        None => Vec::new(),
    }
}

pub fn shape_at(shape: u64) -> Option<Schema> {
    let shapes: Vec<Schema> = serde_json::from_str(SHAPES).unwrap_or_default();
    match shapes.len() as u64 {
        0 => None,
        n => Some(shapes[(shape % n) as usize].clone()),
    }
}

/// @implements REQ-DRT-GEN.seed_reproduces
pub fn value(schema: &Schema, rng: &mut Rng) -> Value {
    match schema {
        Schema::Nat { max, edges } => {
            if !edges.is_empty() && rng.chance(40) {
                return Value::from(edges[rng.below(edges.len() as u64) as usize]);
            }
            let bound = max.unwrap_or(1_000);
            Value::from(rng.below(bound.saturating_add(1)))
        }
        Schema::Int { min, max } => {
            let lo = min.unwrap_or(-1_000);
            let hi = max.unwrap_or(1_000);
            let span = (hi - lo).unsigned_abs().saturating_add(1);
            Value::from(lo + rng.below(span) as i64)
        }
        Schema::Bool => Value::from(rng.chance(50)),
        Schema::Str { max_len, examples } => {
            if !examples.is_empty() && rng.chance(50) {
                return Value::from(examples[rng.below(examples.len() as u64) as usize].clone());
            }
            let len = rng.below(max_len.unwrap_or(12) as u64 + 1) as usize;
            let s: String = (0..len)
                .map(|_| (b'a' + rng.below(26) as u8) as char)
                .collect();
            Value::from(s)
        }
        Schema::Option { inner } => {
            if rng.chance(25) { Value::Null } else { value(inner, rng) }
        }
        Schema::List { inner, max_len } => {
            let len = rng.below(max_len.unwrap_or(6) as u64 + 1) as usize;
            Value::Array((0..len).map(|_| value(inner, rng)).collect())
        }
        Schema::Struct { fields } => {
            let mut map = Map::new();
            for (name, field) in fields {
                map.insert(name.clone(), value(field, rng));
            }
            Value::Object(map)
        }
        Schema::Tuple { items } => {
            Value::Array(items.iter().map(|item| value(item, rng)).collect())
        }
        Schema::Enum { variants } => {
            let names: Vec<&String> = variants.keys().collect();
            if names.is_empty() {
                return Value::Null;
            }
            let pick = names[rng.below(names.len() as u64) as usize];
            match &variants[pick] {
                None => Value::from(pick.clone()),
                Some(payload) => {
                    let mut map = Map::new();
                    map.insert(pick.clone(), value(payload, rng));
                    Value::Object(map)
                }
            }
        }
    }
}

/// @implements REQ-DRT-GEN.shrink_terminates
pub fn shrink(schema: &Schema, v: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    match (schema, v) {
        (Schema::Nat { .. }, Value::Number(n)) => {
            if let Some(k) = n.as_u64() {
                if k > 0 {
                    out.push(Value::from(0u64));
                    if k > 1 {
                        out.push(Value::from(k / 2));
                        out.push(Value::from(k - 1));
                    }
                }
            }
        }
        (Schema::Int { .. }, Value::Number(n)) => {
            if let Some(k) = n.as_i64() {
                if k != 0 {
                    out.push(Value::from(0i64));
                    out.push(Value::from(k / 2));
                }
            }
        }
        (Schema::Bool, Value::Bool(true)) => out.push(Value::from(false)),
        (Schema::Str { .. }, Value::String(s)) => {
            if !s.is_empty() {
                out.push(Value::from(""));
                out.push(Value::from(s.chars().take(s.chars().count() / 2).collect::<String>()));
            }
        }
        (Schema::Option { .. }, v) if !v.is_null() => out.push(Value::Null),
        (Schema::List { inner, .. }, Value::Array(items)) => {
            if !items.is_empty() {
                out.push(Value::Array(vec![]));
                for i in 0..items.len() {
                    let mut shorter = items.clone();
                    shorter.remove(i);
                    out.push(Value::Array(shorter));
                }
                for (i, item) in items.iter().enumerate() {
                    for smaller in shrink(inner, item) {
                        let mut next = items.clone();
                        next[i] = smaller;
                        out.push(Value::Array(next));
                    }
                }
            }
        }
        (Schema::Tuple { items }, Value::Array(values)) if items.len() == values.len() => {
            for (i, (item, current)) in items.iter().zip(values).enumerate() {
                for smaller in shrink(item, current) {
                    let mut next = values.clone();
                    next[i] = smaller;
                    out.push(Value::Array(next));
                }
            }
        }
        (Schema::Struct { fields }, Value::Object(map)) => {
            for (name, field) in fields {
                let Some(current) = map.get(name) else { continue };
                for smaller in shrink(field, current) {
                    let mut next = map.clone();
                    next.insert(name.clone(), smaller);
                    out.push(Value::Object(next));
                }
            }
        }
        (Schema::Enum { variants }, Value::Object(map)) => {
            for (name, payload) in variants {
                let (Some(inner), Some(current)) = (payload, map.get(name)) else { continue };
                for smaller in shrink(inner, current) {
                    let mut next = map.clone();
                    next.insert(name.clone(), smaller);
                    out.push(Value::Object(next));
                }
            }
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// @tests REQ-DRT-GEN.seed_reproduces
    #[test]
    fn a_seed_reproduces_its_cases() {
        let schema = Schema::List {
            inner: Box::new(Schema::Nat { max: Some(50), edges: vec![7] }),
            max_len: Some(5),
        };
        let run = |seed| {
            let mut rng = Rng::new(seed);
            (0..40).map(|_| value(&schema, &mut rng)).collect::<Vec<_>>()
        };
        assert_eq!(run(7), run(7));
        assert_ne!(run(7), run(8));
    }

    /// @tests REQ-DRT-GEN.edges_sampled
    #[test]
    fn declared_edges_are_sampled_far_above_uniform() {
        let schema = Schema::Nat { max: Some(100_000), edges: vec![5_000] };
        let mut rng = Rng::new(1);
        let hits = (0..1_000)
            .filter(|_| value(&schema, &mut rng) == serde_json::json!(5_000))
            .count();
        assert!(hits > 250, "edge sampled only {hits} times in 1000");
    }

    /// @tests REQ-DRT-GEN.shrink_terminates
    /// @structural REQ-DRT-GEN.shrink_terminates reason="a claim that every reduction step is strictly smaller, quantified over the schema grammar; the model has no Schema to quantify over"
    #[test]
    fn shrinking_always_reaches_a_fixed_point() {
        let schema = Schema::List {
            inner: Box::new(Schema::Nat { max: Some(1_000), edges: vec![] }),
            max_len: Some(6),
        };
        let mut rng = Rng::new(3);
        for _ in 0..50 {
            let mut current = value(&schema, &mut rng);
            let mut steps = 0;
            while let Some(next) = shrink(&schema, &current).into_iter().next() {
                current = next;
                steps += 1;
                assert!(steps < 500, "shrinking did not converge");
            }
        }
    }

    /// @tests REQ-DRT-GEN.shrink_terminates
    #[test]
    fn every_kind_of_value_shrinks_toward_its_smallest() {
        let int = Schema::Int { min: None, max: None };
        assert_eq!(shrink(&int, &Value::from(-9)), vec![Value::from(0), Value::from(-4)]);
        assert!(shrink(&int, &Value::from(0)).is_empty());
        assert!(shrink(&Schema::Nat { max: None, edges: vec![] }, &Value::from(-3)).is_empty());

        let pair = Schema::Tuple { items: vec![Schema::Bool, Schema::Nat { max: None, edges: vec![] }] };
        assert_eq!(
            shrink(&pair, &serde_json::json!([true, 1])),
            vec![serde_json::json!([false, 1]), serde_json::json!([true, 0])]
        );
        assert!(shrink(&pair, &serde_json::json!([true])).is_empty());

        let mut variants = std::collections::BTreeMap::new();
        variants.insert("none".to_string(), None);
        variants.insert("some".to_string(), Some(Box::new(Schema::Bool)));
        let either = Schema::Enum { variants };
        assert_eq!(shrink(&either, &serde_json::json!({"some": true})), vec![serde_json::json!({"some": false})]);
        assert!(shrink(&either, &Value::from("none")).is_empty());
    }

    /// @tests REQ-DRT-GEN.shrink_terminates
    #[test]
    fn a_string_of_wide_characters_shrinks_by_characters() {
        let schema = Schema::Str { max_len: Some(3), examples: vec![] };
        let smaller = shrink(&schema, &Value::from("éé"));
        assert_eq!(smaller, vec![Value::from(""), Value::from("é")]);
    }

    #[test]
    fn enum_encodes_nullary_variants_as_bare_strings() {
        let schema = Schema::simple_enum(&["L1", "L4"]);
        let mut rng = Rng::new(2);
        for _ in 0..20 {
            let v = value(&schema, &mut rng);
            assert!(v == serde_json::json!("L1") || v == serde_json::json!("L4"), "{v}");
        }
    }

    #[test]
    fn struct_generates_every_declared_field() {
        let mut fields = BTreeMap::new();
        fields.insert("bond".to_string(), Schema::simple_enum(&["modelImpl"]));
        fields.insert("level".to_string(), Schema::simple_enum(&["L2"]));
        let mut rng = Rng::new(5);
        let v = value(&Schema::Struct { fields }, &mut rng);
        assert_eq!(v, serde_json::json!({"bond": "modelImpl", "level": "L2"}));
    }
}
