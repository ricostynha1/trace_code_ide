//! What a sandboxed agent's usage is estimated to have cost.
//!
//! Implements `REQ-COST`. Usage records the tool wrote about itself, priced
//! against a table this project keeps by hand. That is all it is, and the
//! requirement is mostly about saying so.
//!
//! Arithmetic over naturals: a price is per million tokens, an amount is whole
//! millionths of a unit of currency. A floating-point total would make two
//! implementations disagree in the last digit for reasons that have nothing to
//! do with pricing.

use serde::{Deserialize, Serialize};

pub use crate::observe::transcript::Usage;

/// What a million tokens of each kind costs, in millionths of a unit.
///
/// Four rates and not one. Cached input is the cheapest thing on the bill and a
/// cache write the dearest, often by an order of magnitude in each direction,
/// so a total that added them at one rate would be wrong by more than rounding
/// — and wrong in whichever direction the workload happened to lean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub model: String,
    pub input: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
}

/// An estimate: what the priced usage came to, and what could not be priced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub amount: u64,
    /// The models the table did not price, named once each, in the order met.
    pub unpriced: Vec<String>,
}

/// The price for a model, if the table has one.
///
/// @implements REQ-COST.price_is_per_model
pub fn price_of(table: Vec<Price>, model: String) -> Option<Price> {
    table.into_iter().find(|price| price.model == model)
}

/// What one usage record costs, at one price.
///
/// Integer division, truncating. The unit is a millionth, so the discarded part
/// is less than a millionth of a unit per line — below anything this figure
/// claims to resolve, and deterministic, which the alternative is not.
///
/// @implements REQ-COST.cache_priced_apart
pub fn amount_of(price: Price, usage: Usage) -> u64 {
    (usage.input * price.input
        + usage.cached * price.cached
        + usage.cache_write * price.cache_write
        + usage.output * price.output)
        / 1_000_000
}

/// What a run of usage records is estimated to have cost.
///
/// A model the table does not price is named rather than counted as free. A
/// total that quietly priced an unknown model at zero would be confidently
/// wrong and look right, which is the failure `ARCH-HONEST` exists to prevent.
///
/// @implements REQ-COST.cost_from_usage
/// @implements REQ-COST.price_is_per_model
/// @drt REQ-COST.cost_from_usage
/// @drt REQ-COST.price_is_per_model
pub fn spend_of(table: Vec<Price>, usage: Vec<Usage>) -> Spend {
    let mut spend = Spend::default();
    for one in usage {
        match price_of(table.clone(), one.model.clone()) {
            Some(price) => spend.amount += amount_of(price, one),
            None => {
                if !spend.unpriced.contains(&one.model) {
                    spend.unpriced.push(one.model);
                }
            }
        }
    }
    spend
}

/// The estimate as a person reads it.
///
/// It says `estimated`, and when anything went unpriced it says that too, in
/// the same line. A figure derived from somebody else's log and a table kept by
/// hand is not a bill, and rendering it as one would be a claim nobody here can
/// back.
///
/// @implements REQ-COST.estimate_is_labelled
/// @drt REQ-COST.estimate_is_labelled
pub fn estimate_line(spend: Spend) -> String {
    let money = format!("{}.{:06}", spend.amount / 1_000_000, spend.amount % 1_000_000);
    let tail = if spend.unpriced.is_empty() {
        String::new()
    } else {
        format!(" (unpriced: {})", spend.unpriced.join(", "))
    };
    format!("estimated {money}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<Price> {
        vec![Price {
            model: "sonnet".into(),
            input: 3_000_000,
            cached: 300_000,
            cache_write: 3_750_000,
            output: 15_000_000,
        }]
    }

    /// @tests REQ-COST.cache_priced_apart
    #[test]
    fn the_three_kinds_of_input_are_priced_at_their_own_rates() {
        let price = table().remove(0);
        // A million of each kind, one kind at a time. If any two shared a rate
        // these would not be four different numbers.
        let only = |input, cached, cache_write, output| {
            amount_of(
                price.clone(),
                Usage { model: "sonnet".into(), input, cached, cache_write, output },
            )
        };
        assert_eq!(only(1_000_000, 0, 0, 0), 3_000_000);
        assert_eq!(only(0, 1_000_000, 0, 0), 300_000);
        assert_eq!(only(0, 0, 1_000_000, 0), 3_750_000);
        assert_eq!(only(0, 0, 0, 1_000_000), 15_000_000);
    }

    /// @tests REQ-COST.price_is_per_model
    /// @tests REQ-COST.cost_from_usage
    #[test]
    fn an_unpriced_model_is_named_rather_than_counted_as_free() {
        let usage = vec![
            Usage { model: "sonnet".into(), input: 1_000_000, cached: 0, cache_write: 0, output: 0 },
            Usage { model: "future".into(), input: 9_999_999, cached: 0, cache_write: 0, output: 0 },
            Usage { model: "future".into(), input: 1, cached: 0, cache_write: 0, output: 0 },
        ];
        let spend = spend_of(table(), usage);
        assert_eq!(spend.amount, 3_000_000, "the unpriced usage did not join the total");
        // Named once, not once per record.
        assert_eq!(spend.unpriced, vec!["future".to_string()]);
    }

    /// @tests REQ-COST.estimate_is_labelled
    #[test]
    fn the_line_says_it_is_an_estimate_and_says_what_it_left_out() {
        assert_eq!(estimate_line(Spend { amount: 0, unpriced: vec![] }), "estimated 0.000000");
        assert_eq!(
            estimate_line(Spend { amount: 1_234_567, unpriced: vec!["future".into()] }),
            "estimated 1.234567 (unpriced: future)"
        );
        // The fractional part is padded, or 1.000005 would read as 1.5.
        assert_eq!(estimate_line(Spend { amount: 1_000_005, unpriced: vec![] }), "estimated 1.000005");
    }
}
