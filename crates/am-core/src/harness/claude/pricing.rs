//! Claude API list prices, for the estimated cost of today's usage
//! (`dailyTokensMode: "cost"`). US dollars per million tokens, first-party API
//! rates. Cache writes cost 1.25× input (5-minute TTL) or 2× (1-hour TTL).
//! An estimate: subscription plans aren't billed per token, and long-context
//! surcharges on older models aren't modelled. Unknown models price as `None`
//! and are reported as unpriced rather than guessed.

use crate::usage::TokenCounts;

/// $/MTok for one model.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Price {
    input: f64,
    output: f64,
    cache_read: f64,
}

const fn p(input: f64, output: f64, cache_read: f64) -> Price {
    Price { input, output, cache_read }
}

/// Model id prefixes, most specific first (`claude-opus-5-5` before
/// `claude-opus-5`). Dated ids (`claude-sonnet-4-5-20250929`) match by prefix.
const PRICES: &[(&str, Price)] = &[
    ("claude-fable-5-1", p(10.0, 50.0, 0.25)),
    ("claude-mythos-5-1", p(10.0, 50.0, 0.25)),
    ("claude-fable-5", p(10.0, 50.0, 1.0)),
    ("claude-mythos-5", p(10.0, 50.0, 1.0)),
    ("claude-opus-5-5", p(4.0, 20.0, 0.20)),
    ("claude-opus-5", p(5.0, 25.0, 0.50)),
    ("claude-opus-4-8", p(5.0, 25.0, 0.50)),
    ("claude-opus-4-7", p(5.0, 25.0, 0.50)),
    ("claude-opus-4-6", p(5.0, 25.0, 0.50)),
    ("claude-opus-4-5", p(5.0, 25.0, 0.50)),
    ("claude-opus-4", p(15.0, 75.0, 1.50)),
    ("claude-sonnet-5", p(2.0, 10.0, 0.20)),
    ("claude-sonnet-4", p(3.0, 15.0, 0.30)),
    ("claude-3-7-sonnet", p(3.0, 15.0, 0.30)),
    ("claude-haiku-5-5", p(0.10, 0.50, 0.01)),
    ("claude-haiku-4-5", p(1.0, 5.0, 0.10)),
    ("claude-3-5-haiku", p(0.80, 4.0, 0.08)),
];

/// Claude Haiku 5.5 prompts over this many tokens are billed at the higher tier.
const HAIKU_5_5_TIER: u64 = 100_000;

fn price(model: &str, prompt_tokens: u64) -> Option<Price> {
    if model.starts_with("claude-haiku-5-5") && prompt_tokens > HAIKU_5_5_TIER {
        return Some(p(0.50, 2.50, 0.05));
    }
    PRICES.iter().find(|(prefix, _)| model.starts_with(prefix)).map(|(_, price)| *price)
}

/// Estimated cost of one response in US dollars; `fast` (fast mode) doubles
/// it. `None` for a model without a known price.
pub fn cost_usd(model: &str, t: &TokenCounts, fast: bool) -> Option<f64> {
    let prompt = t.input + t.cache_write() + t.cache_read;
    let pr = price(model, prompt)?;
    let micro = t.input as f64 * pr.input
        + t.cache_write_5m as f64 * pr.input * 1.25
        + t.cache_write_1h as f64 * pr.input * 2.0
        + t.cache_read as f64 * pr.cache_read
        + t.output as f64 * pr.output;
    Some(micro / 1_000_000.0 * if fast { 2.0 } else { 1.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(input: u64, w5: u64, w1h: u64, read: u64, output: u64) -> TokenCounts {
        TokenCounts { input, cache_write_5m: w5, cache_write_1h: w1h, cache_read: read, output }
    }

    #[test]
    fn prices_every_class() {
        // Opus 5.5: 1M in = $4, 1M 5m write = $5, 1M 1h write = $8, 1M read = $0.20, 1M out = $20.
        let m = 1_000_000;
        let c = cost_usd("claude-opus-5-5", &counts(m, m, m, m, m), false).unwrap();
        assert!((c - 37.2).abs() < 1e-9, "{c}");
        assert!((cost_usd("claude-opus-5-5", &counts(m, 0, 0, 0, 0), true).unwrap() - 8.0).abs() < 1e-9);
    }

    #[test]
    fn matches_the_most_specific_prefix_and_dated_ids() {
        let one_in = counts(1_000_000, 0, 0, 0, 0);
        assert_eq!(cost_usd("claude-opus-5-5", &one_in, false), Some(4.0));
        assert_eq!(cost_usd("claude-opus-5", &one_in, false), Some(5.0));
        assert_eq!(cost_usd("claude-opus-4-1-20250805", &one_in, false), Some(15.0));
        assert_eq!(cost_usd("claude-sonnet-4-5-20250929", &one_in, false), Some(3.0));
        assert_eq!(cost_usd("gpt-5", &one_in, false), None);
    }

    #[test]
    fn haiku_5_5_long_prompts_use_the_higher_tier() {
        let short = cost_usd("claude-haiku-5-5", &counts(100_000, 0, 0, 0, 0), false).unwrap();
        let long = cost_usd("claude-haiku-5-5", &counts(200_000, 0, 0, 0, 0), false).unwrap();
        assert!((short - 0.01).abs() < 1e-9 && (long - 0.1).abs() < 1e-9, "{short} {long}");
    }
}
