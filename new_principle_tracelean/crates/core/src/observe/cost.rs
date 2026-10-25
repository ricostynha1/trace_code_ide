use serde::{Deserialize, Serialize};

pub use crate::observe::transcript::Usage;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub model: String,
    pub input: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub amount: u64,
    pub unpriced: Vec<String>,
}

/// @implements REQ-COST.price_is_per_model
pub fn price_of(table: Vec<Price>, model: String) -> Option<Price> {
    table.into_iter().find(|price| price.model == model)
}

/// @implements REQ-COST.cache_priced_apart
pub fn amount_of(price: Price, usage: Usage) -> u64 {
    (usage.input * price.input
        + usage.cached * price.cached
        + usage.cache_write * price.cache_write
        + usage.output * price.output)
        / 1_000_000
}

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
        assert_eq!(estimate_line(Spend { amount: 1_000_005, unpriced: vec![] }), "estimated 1.000005");
    }
}
