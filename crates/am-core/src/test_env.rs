//! Tests that point `AM_DATA_DIR` at a temp dir hold [`lock`] for their whole
//! run; [`data_dir`] restores the variable (and the plugin registry) on drop.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

pub fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct DataDir(Option<std::ffi::OsString>);

/// Set `AM_DATA_DIR` to `dir` until the guard drops. Hold [`lock`] first.
pub fn data_dir(dir: &Path) -> DataDir {
    let prev = std::env::var_os("AM_DATA_DIR");
    // SAFETY: tests that touch the environment are serialised by `lock`.
    unsafe { std::env::set_var("AM_DATA_DIR", dir) };
    crate::harness::reload();
    DataDir(prev)
}

impl Drop for DataDir {
    fn drop(&mut self) {
        // SAFETY: as above.
        match self.0.take() {
            Some(v) => unsafe { std::env::set_var("AM_DATA_DIR", v) },
            None => unsafe { std::env::remove_var("AM_DATA_DIR") },
        }
        crate::harness::reload();
    }
}
