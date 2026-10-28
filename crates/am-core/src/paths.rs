use std::path::PathBuf;

/// `~/.agent-monitor`, or `$AM_DATA_DIR` when set (test isolation).
pub fn app_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("AM_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    home_dir().join(".agent-monitor")
}

/// `$HOME`, else the user's passwd entry (a hook run with a scrubbed
/// environment, e.g. by Muse Code), else `/`. An empty `$HOME` counts as unset.
pub fn home_dir() -> PathBuf {
    home_from(std::env::var_os("HOME"))
}

fn home_from(var: Option<std::ffi::OsString>) -> PathBuf {
    var.filter(|h| !h.is_empty()).map(PathBuf::from).or_else(passwd_home).unwrap_or_else(|| PathBuf::from("/"))
}

/// The current user's home directory from the passwd database.
fn passwd_home() -> Option<PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: a zeroed `passwd` is a valid out-parameter for getpwuid_r.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: getpwuid_r writes into `pwd` and `buf`, which outlive the reads below.
    let rc = unsafe { libc::getpwuid_r(libc::getuid(), &mut pwd, buf.as_mut_ptr(), buf.len(), &mut out) };
    if rc != 0 || out.is_null() || pwd.pw_dir.is_null() {
        return None;
    }
    // SAFETY: `pw_dir` points into `buf` and is NUL-terminated.
    let dir = unsafe { CStr::from_ptr(pwd.pw_dir) }.to_bytes();
    (!dir.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(dir)))
}

pub fn db_path() -> PathBuf {
    app_dir().join("agent-monitor.db")
}

pub fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

pub fn log_path() -> PathBuf {
    app_dir().join("debug.log")
}

/// Where `log::rotate_if_needed` moves the full log: `debug.log.1`.
pub fn rotated_log_path() -> PathBuf {
    log_path().with_extension("log.1")
}

/// The log files that exist, `debug.log` first.
pub fn log_files() -> Vec<PathBuf> {
    [log_path(), rotated_log_path()].into_iter().filter(|p| p.is_file()).collect()
}

pub fn socket_path() -> PathBuf {
    app_dir().join("am.sock")
}

pub fn capture_dir() -> PathBuf {
    app_dir().join("hook-captures")
}

/// Custom themes, one `<name>.json` each.
pub fn themes_dir() -> PathBuf {
    app_dir().join("themes")
}

/// Last update check (`update_check`), shared by every running TUI.
pub fn update_check_path() -> PathBuf {
    app_dir().join("update-check.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_passwd_entry_gives_a_home_without_env() {
        let pw = passwd_home().expect("the test user has a passwd entry");
        assert!(pw.is_absolute(), "{}", pw.display());
        // Unset or empty $HOME falls back to it; a set one wins.
        assert_eq!(home_from(None), pw);
        assert_eq!(home_from(Some("".into())), pw);
        assert_eq!(home_from(Some("/x/home".into())), PathBuf::from("/x/home"));
    }
}
