//! Moving an editor's window to a chosen monitor (the `ideDisplay` setting).
//!
//! No editor CLI takes a display, and VS Code-family (Electron) apps don't
//! answer AppleScript `bounds`, so this goes through System Events: one JXA
//! call lists the screens and waits for the editor's window for the worktree,
//! the position is worked out here ([`target_rect`], pure and tested), and a
//! second call moves it. System Events needs Accessibility permission for the
//! terminal am runs in; without it the editor still opens and the caller is
//! told how to grant it ([`MoveError::Permission`]). macOS only.

use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::{debug, info, warn};

/// One display, as `NSScreen` reports it (Cocoa coordinates: origin at the
/// bottom left of the main screen, y up). `v*` is the visible frame (without
/// the menu bar and Dock).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Screen {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub vx: f64,
    pub vy: f64,
    pub vw: f64,
    pub vh: f64,
}

/// The editor window the find step settled on (System Events coordinates).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Window {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Deserialize)]
struct Found {
    screens: Vec<Screen>,
    window: Option<Window>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveError {
    /// System Events refused: the terminal needs Accessibility permission.
    Permission(String),
    Other(String),
}

impl MoveError {
    pub fn message(&self) -> &str {
        match self {
            MoveError::Permission(m) | MoveError::Other(m) => m,
        }
    }
}

/// Screens in the order `ideDisplay` counts them: left to right, then top to
/// bottom for displays stacked at the same x.
pub fn ordered(screens: &[Screen]) -> Vec<Screen> {
    let mut v = screens.to_vec();
    // Cocoa y grows upwards, so the top of a screen is y + h.
    v.sort_by(|a, b| a.x.total_cmp(&b.x).then((b.y + b.h).total_cmp(&(a.y + a.h))));
    v
}

/// Where (System Events coordinates: top left of the main screen, y down)
/// and how big a `win`-sized window should be to sit centred on display `n`
/// (1-based, [`ordered`]), shrunk to fit its visible frame. `None` when
/// there is no display `n`. `screens[0]` is the main screen, as `NSScreen.screens` gives it.
pub fn target_rect(screens: &[Screen], n: u8, win: (f64, f64)) -> Option<(i64, i64, i64, i64)> {
    let main = screens.first()?;
    let t = *ordered(screens).get(usize::from(n).checked_sub(1)?)?;
    let top = main.h - (t.vy + t.vh);
    let (w, h) = (win.0.min(t.vw), win.1.min(t.vh));
    let x = t.vx + (t.vw - w) / 2.0;
    let y = top + (t.vh - h) / 2.0;
    Some((x.round() as i64, y.round() as i64, w.round() as i64, h.round() as i64))
}

/// Lists the screens, then waits (up to `argv[2]` ms) for a window of
/// process `argv[0]` whose title contains `argv[1]`. Prints JSON `Found`.
const FIND_JS: &str = r#"ObjC.import('AppKit');
function run(argv) {
  const proc = argv[0], folder = argv[1], wait = parseInt(argv[2], 10);
  const screens = [], list = $.NSScreen.screens;
  for (let i = 0; i < list.count; i++) {
    const s = list.objectAtIndex(i), f = s.frame, v = s.visibleFrame;
    screens.push({x: f.origin.x, y: f.origin.y, w: f.size.width, h: f.size.height,
                  vx: v.origin.x, vy: v.origin.y, vw: v.size.width, vh: v.size.height});
  }
  const se = Application('System Events');
  const end = Date.now() + wait;
  let window = null;
  while (true) {
    const ps = se.processes.whose({name: proc});
    if (ps.length > 0) {
      const ws = ps[0].windows();
      for (const w of ws) {
        const name = w.name() || '';
        if (name.indexOf(folder) >= 0) {
          const p = w.position(), z = w.size();
          window = {name: name, x: p[0], y: p[1], w: z[0], h: z[1]};
          break;
        }
      }
    }
    if (window || Date.now() >= end) break;
    delay(0.2);
  }
  return JSON.stringify({screens: screens, window: window});
}"#;

/// Moves the window of process `argv[0]` titled `argv[1]` to `argv[2..6]`
/// (x, y, w, h).
const MOVE_JS: &str = r#"function run(argv) {
  const se = Application('System Events');
  const w = se.processes.whose({name: argv[0]})[0].windows.whose({name: argv[1]})[0];
  const r = argv.slice(2).map(Number);
  w.position = [r[0], r[1]];
  w.size = [r[2], r[3]];
  w.position = [r[0], r[1]];
  return 'ok';
}"#;

/// How long the editor gets to show the worktree's window.
const WAIT_MS: u32 = 8_000;

fn osascript(script: &str, args: &[String]) -> Result<String, MoveError> {
    let out = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", script])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| MoveError::Other(format!("couldn't run osascript: {e}")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if is_permission_error(&err) {
        let app = match std::env::var("TERM_PROGRAM").ok().as_deref() {
            Some("vscode") => "your editor",
            _ => crate::launch::terminal_app(),
        };
        MoveError::Permission(format!(
            "allow {app} in System Settings → Privacy & Security → Accessibility so am can move the editor window"
        ))
    } else {
        MoveError::Other(err.lines().last().unwrap_or("osascript failed").to_string())
    })
}

fn is_permission_error(stderr: &str) -> bool {
    ["-1719", "-25211", "1002", "assistive access", "not allowed"].iter().any(|m| stderr.contains(m))
}

/// Wait for `process`'s window titled with `folder` and move it to display
/// `n`. `Ok(())` when it was moved (or already there).
pub fn move_to_display(process: &str, folder: &str, n: u8) -> Result<(), MoveError> {
    if !cfg!(target_os = "macos") {
        debug!("window", "ideDisplay {n} ignored: moving windows is macOS only");
        return Ok(());
    }
    let result = move_inner(process, folder, n);
    match &result {
        Ok(()) => info!("window", "Moved the {process} window for {folder} to display {n}"),
        Err(e) => log_failure(process, folder, n, e),
    }
    result
}

fn log_failure(process: &str, folder: &str, n: u8, e: &MoveError) {
    let kind = match e {
        MoveError::Permission(_) => "accessibility_denied",
        MoveError::Other(_) => "move_window_failed",
    };
    warn!("window", kind = kind, "Couldn't move the {process} window for {folder} to display {n}: {}", e.message());
}

fn move_inner(process: &str, folder: &str, n: u8) -> Result<(), MoveError> {
    let raw = osascript(FIND_JS, &[process.into(), folder.into(), WAIT_MS.to_string()])?;
    let found: Found = serde_json::from_str(&raw).map_err(|e| MoveError::Other(format!("unexpected answer from osascript: {e}")))?;
    debug!("window", "{} screen(s), window {:?}", found.screens.len(), found.window);
    let Some(win) = found.window else {
        return Err(MoveError::Other(format!("no {process} window for {folder} appeared")));
    };
    let Some((x, y, w, h)) = target_rect(&found.screens, n, (win.w, win.h)) else {
        return Err(MoveError::Other(format!("display {n} not found ({} connected)", found.screens.len())));
    };
    if (x, y, w, h) == (win.x.round() as i64, win.y.round() as i64, win.w.round() as i64, win.h.round() as i64) {
        return Ok(());
    }
    osascript(MOVE_JS, &[process.into(), win.name, x.to_string(), y.to_string(), w.to_string(), h.to_string()]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(x: f64, y: f64, w: f64, h: f64) -> Screen {
        // A 25-pixel menu bar at the top, nothing else hidden.
        Screen { x, y, w, h, vx: x, vy: y, vw: w, vh: h - 25.0 }
    }

    #[test]
    fn displays_count_left_to_right() {
        // Main 1440x900 on the right, a 1920x1080 to its left, lower.
        let main = screen(0.0, 0.0, 1440.0, 900.0);
        let left = screen(-1920.0, -180.0, 1920.0, 1080.0);
        let screens = [main, left];
        assert_eq!(ordered(&screens), vec![left, main]);
        // Display 1 = the left one: its top is 180 below the main's in SE coordinates.
        // Visible top = 900 - (-180 + 1055) = 25; height 1055.
        assert_eq!(target_rect(&screens, 1, (1000.0, 800.0)), Some((-1460, 153, 1000, 800)));
        // Display 2 = main.
        assert_eq!(target_rect(&screens, 2, (1000.0, 800.0)), Some((220, 63, 1000, 800)));
        assert_eq!(target_rect(&screens, 3, (1000.0, 800.0)), None);
        assert_eq!(target_rect(&screens, 0, (1000.0, 800.0)), None);
    }

    #[test]
    fn stacked_displays_count_top_to_bottom_and_windows_shrink_to_fit() {
        let main = screen(0.0, 0.0, 1440.0, 900.0);
        let above = screen(0.0, 900.0, 1440.0, 900.0);
        let screens = [main, above];
        assert_eq!(ordered(&screens), vec![above, main]);
        // Above the main screen: SE y is negative. Too big a window is shrunk.
        assert_eq!(target_rect(&screens, 1, (3000.0, 3000.0)), Some((0, -875, 1440, 875)));
    }

    #[test]
    fn permission_errors_are_recognised() {
        assert!(is_permission_error("execution error: System Events got an error: osascript is not allowed assistive access. (-1719)"));
        assert!(!is_permission_error("execution error: Can't get object. (-1728)"));
    }
}
