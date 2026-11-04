// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! terminumd's own files are the user's alone (umask 077, AC-09); what it makes for the user keeps
//! the umask it was started with: the tmux server it starts (so every terminal and what runs in
//! it) and the files and folders Files makes. Without this every new file of a terminal was 0600.

use std::sync::atomic::{AtomicU32, Ordering};

const UNSET: u32 = u32::MAX;
/// The umask before `private`, kept for the user's processes and files.
static STARTED: AtomicU32 = AtomicU32::new(UNSET);

/// Private files from here on; the umask before is kept (the first call's, when called again).
pub fn private() {
    // SAFETY: umask has no preconditions
    let was = unsafe { libc::umask(0o077) } as u32;
    let _ = STARTED.compare_exchange(UNSET, was, Ordering::Relaxed, Ordering::Relaxed);
}

/// The umask the process was started with, once `private` ran.
fn started() -> Option<u32> {
    let m = STARTED.load(Ordering::Relaxed);
    (m != UNSET).then_some(m)
}

/// In a child before exec (`pre_exec`): the umask the process was started with. Only an atomic
/// load and umask(2), both async-signal-safe.
pub fn restore() {
    if let Some(m) = started() {
        // SAFETY: umask has no preconditions
        unsafe { libc::umask(m as libc::mode_t) };
    }
}

/// The mode a new file (`dir` false) or folder of the user's gets: as its creation under the
/// started umask would give it (022 when none was kept).
pub fn user_mode(dir: bool) -> u32 {
    (if dir { 0o777 } else { 0o666 }) & !started().unwrap_or(0o022)
}

/// Tests: the started umask as if `private` had found `m` (the process's own is not changed).
#[cfg(test)]
pub fn set_started(m: u32) {
    STARTED.store(m, Ordering::Relaxed);
}

#[cfg(test)]
#[path = "umask_tests.rs"]
mod tests;
