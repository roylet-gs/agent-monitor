//! Desktop notifications through the terminal: OSC 9 (iTerm2, Ghostty,
//! WezTerm, kitty) or OSC 777 (Warp). Inside tmux the sequence is wrapped in
//! a DCS passthrough so it reaches the outer terminal (needs
//! `allow-passthrough on`, which the tmux plugin sets). Terminals that don't
//! understand them (Terminal.app, VS Code) ignore them.

use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Osc9,
    Osc777,
}

pub fn flavor_from_env() -> Flavor {
    match std::env::var("TERM_PROGRAM").as_deref() {
        Ok("WarpTerminal") => Flavor::Osc777,
        _ => Flavor::Osc9,
    }
}

/// Control characters would end the sequence early; `;` at the start of an
/// OSC 9 body is how ConEmu extensions (`9;4` progress) are spelled.
fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim_start_matches(|c: char| c.is_ascii_digit() || c == ';').to_string()
}

pub fn encode(title: &str, body: &str, flavor: Flavor, tmux: bool) -> String {
    let seq = match flavor {
        Flavor::Osc9 => format!("\x1b]9;{}\x07", clean(&format!("{title}: {body}"))),
        Flavor::Osc777 => format!("\x1b]777;notify;{};{}\x07", clean(title).replace(';', ","), clean(body)),
    };
    if tmux { format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b")) } else { seq }
}

/// One line per batch of status changes, e.g. `feat-x is waiting for you`.
pub fn message(waiting: &[String], done: &[String]) -> Option<String> {
    let list = |names: &[String]| match names {
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, rest @ ..] => format!("{a} and {} more", rest.len()),
        [] => String::new(),
    };
    let mut parts = Vec::new();
    if !waiting.is_empty() {
        parts.push(format!("{} {} waiting for you", list(waiting), if waiting.len() == 1 { "is" } else { "are" }));
    }
    if !done.is_empty() {
        parts.push(format!("{} finished", list(done)));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

pub fn send(body: &str) {
    let tmux = std::env::var_os("TMUX").is_some_and(|v| !v.is_empty());
    let seq = encode("agent-monitor", body, flavor_from_env(), tmux);
    let mut out = std::io::stdout();
    match out.write_all(seq.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => am_core::info!("notify", "Sent terminal notification: {body}"),
        Err(e) => am_core::warn!("notify", "Couldn't write terminal notification: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc9_and_777() {
        assert_eq!(encode("am", "x is waiting", Flavor::Osc9, false), "\x1b]9;am: x is waiting\x07");
        assert_eq!(encode("a;m", "bell\x07 done", Flavor::Osc777, false), "\x1b]777;notify;a,m;bell done\x07");
        assert_eq!(encode("", "4;1 hi", Flavor::Osc9, false), "\x1b]9;: 4;1 hi\x07");
        assert_eq!(clean("4;1 hi"), " hi", "can't be read as a ConEmu 9;4 progress command");
    }

    #[test]
    fn tmux_passthrough_doubles_escapes() {
        assert_eq!(encode("am", "hi", Flavor::Osc9, true), "\x1bPtmux;\x1b\x1b]9;am: hi\x07\x1b\\");
    }

    #[test]
    fn messages() {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(message(&s(&["a"]), &[]).unwrap(), "a is waiting for you");
        assert_eq!(message(&s(&["a", "b"]), &s(&["c"])).unwrap(), "a and b are waiting for you; c finished");
        assert_eq!(message(&[], &s(&["a", "b", "c"])).unwrap(), "a and 2 more finished");
        assert_eq!(message(&[], &[]), None);
    }
}
