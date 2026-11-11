//! Thread priorities (Windows): the UI thread a step above normal, the
//! workers that build drawings and scenes a step below, so a busy machine
//! (a compile, a screen recording, the workers themselves on every core)
//! still leaves the window a core to answer the pointer with. Elsewhere,
//! nothing changes.

#[cfg(windows)]
mod win {
    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetCurrentThread() -> isize;
        pub fn SetThreadPriority(thread: isize, priority: i32) -> i32;
    }
}

#[cfg(windows)]
fn set(priority: i32) {
    // SAFETY: the pseudo handle of the calling thread, always valid
    unsafe {
        win::SetThreadPriority(win::GetCurrentThread(), priority);
    }
}

/// The calling thread draws the window: a step above normal.
pub fn ui() {
    #[cfg(windows)]
    set(1); // THREAD_PRIORITY_ABOVE_NORMAL
}

/// The calling thread builds in the background: a step below normal.
pub fn background() {
    #[cfg(windows)]
    set(-1); // THREAD_PRIORITY_BELOW_NORMAL
}
