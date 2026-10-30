//! How full an agent's context window is: the shared measure and display
//! helpers. Each harness plugin reads its own transcripts
//! (`Harness::context_usage`); Claude Code's reader is
//! `harness::claude::transcript`.
//!
//! The app doesn't assume any model, so the window is the one the harness
//! recorded with the usage (Codex writes `model_context_window`), else the
//! `contextWindowTokens` setting, doubled until it holds a session that has
//! gone past it.

/// Smallest `contextWindowTokens` the editors accept.
pub const MIN_WINDOW: u64 = 1_000;
/// Fraction of the window used at which the bar turns yellow, then red.
pub const WARN_AT: f64 = 0.70;
pub const CRITICAL_AT: f64 = 0.90;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextUsage {
    /// Tokens in the context at the last main-chain turn.
    pub tokens: u64,
    pub model: Option<String>,
    /// The window as the harness recorded it, when it does.
    pub window: Option<u64>,
}

impl ContextUsage {
    /// The window to measure this usage against: the recorded one, else
    /// [`window_for`] the setting.
    pub fn window(&self, configured: u64) -> u64 {
        match self.window {
            Some(w) if w >= self.tokens && w > 0 => w,
            _ => window_for(self.tokens, configured),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Critical,
}

/// The window `tokens` are measured against: `configured`, doubled until it
/// holds them (200k → 400k → 800k …), so a session past the setting still gets
/// a meaningful bar whatever model it runs.
pub fn window_for(tokens: u64, configured: u64) -> u64 {
    let mut window = configured.max(1);
    while tokens > window {
        window = window.saturating_mul(2);
    }
    window
}

/// A token count as typed: `128000`, `200k`, `1m`, `1.5M` (`_`/`,` allowed as
/// separators). `None` for anything else, zero included.
pub fn parse_tokens(s: &str) -> Option<u64> {
    let t: String = s.trim().chars().filter(|c| *c != '_' && *c != ',').collect::<String>().to_ascii_lowercase();
    let (num, mult) = match t.strip_suffix('k') {
        Some(n) => (n, 1_000.0),
        None => match t.strip_suffix('m') {
            Some(n) => (n, 1_000_000.0),
            None => (t.as_str(), 1.0),
        },
    };
    let n: f64 = num.trim().parse().ok()?;
    let v = (n * mult).round();
    (1.0..9.0e15).contains(&v).then_some(v as u64)
}

pub fn fraction(tokens: u64, window: u64) -> f64 {
    (tokens as f64 / window.max(1) as f64).clamp(0.0, 1.0)
}

pub fn level(tokens: u64, window: u64) -> Level {
    let f = fraction(tokens, window);
    if f >= CRITICAL_AT {
        Level::Critical
    } else if f >= WARN_AT {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// Compact token count in at most 4 chars: `950`, `12k`, `380k`, `1.2M`, `12M`.
pub fn short_tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_499 => format!("{}k", (n + 500) / 1_000),
        _ => {
            let m = n as f64 / 1_000_000.0;
            if m < 9.95 { format!("{m:.1}M") } else { format!("{}M", m.round() as u64) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_doubles_past_the_setting() {
        assert_eq!(window_for(150_000, 200_000), 200_000);
        assert_eq!(window_for(200_000, 200_000), 200_000);
        assert_eq!(window_for(250_000, 200_000), 400_000);
        assert_eq!(window_for(900_000, 200_000), 1_600_000);
        assert_eq!(window_for(10, 1_000_000), 1_000_000);
        assert_eq!(window_for(5, 0), 8);
        assert_eq!(window_for(u64::MAX, 3), u64::MAX);
    }

    #[test]
    fn a_recorded_window_beats_the_setting_unless_the_session_outgrew_it() {
        let u = |tokens, window| ContextUsage { tokens, model: None, window };
        assert_eq!(u(60_000, Some(272_000)).window(1_000_000), 272_000);
        assert_eq!(u(60_000, None).window(200_000), 200_000);
        assert_eq!(u(300_000, Some(272_000)).window(200_000), 400_000);
        assert_eq!(u(60_000, Some(0)).window(200_000), 200_000);
    }

    #[test]
    fn parses_typed_token_counts() {
        assert_eq!(parse_tokens("128000"), Some(128_000));
        assert_eq!(parse_tokens("200k"), Some(200_000));
        assert_eq!(parse_tokens(" 1M "), Some(1_000_000));
        assert_eq!(parse_tokens("1.5m"), Some(1_500_000));
        assert_eq!(parse_tokens("1_000_000"), Some(1_000_000));
        assert_eq!(parse_tokens("200,000"), Some(200_000));
        for bad in ["", "abc", "0", "-5", "k", "1g", "1e30"] {
            assert_eq!(parse_tokens(bad), None, "{bad}");
        }
    }

    #[test]
    fn levels_and_short_counts() {
        assert_eq!(level(100_000, 200_000), Level::Ok);
        assert_eq!(level(140_000, 200_000), Level::Warn);
        assert_eq!(level(180_000, 200_000), Level::Critical);
        assert_eq!(level(900_000, 200_000), Level::Critical);
        assert_eq!(short_tokens(950), "950");
        assert_eq!(short_tokens(12_400), "12k");
        assert_eq!(short_tokens(380_200), "380k");
        assert_eq!(short_tokens(999_700), "1.0M");
        assert_eq!(short_tokens(1_240_000), "1.2M");
        assert_eq!(short_tokens(12_000_000), "12M");
    }
}
