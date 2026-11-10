//! Alt+A (select nothing, as Blender) kept for CreaBIM while its window is
//! in front (MU39): another program's global hotkey on the same keys (a
//! chat app's screenshot) would otherwise take it first. A low-level
//! keyboard hook sees the keys before hotkeys are matched; while CreaBIM
//! is the foreground window it keeps Alt+A and tells the app, and leaves
//! every other key and every other window alone. Off in Settings › Keymap.

use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(true);
static PRESSED: AtomicBool = AtomicBool::new(false);
static CTX: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();

/// Keep Alt+A for CreaBIM (true) or let it through.
pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Was Alt+A kept since the last call?
pub fn take_alt_a() -> bool {
    PRESSED.swap(false, Ordering::Relaxed)
}

#[cfg(windows)]
pub fn install(ctx: &eframe::egui::Context) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_KEYBOARD_LL};
    let _ = CTX.set(ctx.clone());
    unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook),
            GetModuleHandleW(std::ptr::null()),
            0,
        );
    }
}

#[cfg(not(windows))]
pub fn install(_ctx: &eframe::egui::Context) {}

#[cfg(windows)]
unsafe extern "system" fn hook(code: i32, w: usize, l: isize) -> isize {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetForegroundWindow, GetWindowThreadProcessId, KBDLLHOOKSTRUCT,
        LLKHF_ALTDOWN, WM_KEYDOWN, WM_SYSKEYDOWN,
    };
    if code >= 0
        && ON.load(Ordering::Relaxed)
        && (w as u32 == WM_SYSKEYDOWN || w as u32 == WM_KEYDOWN)
    {
        let k = unsafe { &*(l as *const KBDLLHOOKSTRUCT) };
        if k.vkCode == 0x41 && k.flags & LLKHF_ALTDOWN != 0 {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
            if pid == unsafe { GetCurrentProcessId() } {
                PRESSED.store(true, Ordering::Relaxed);
                if let Some(c) = CTX.get() {
                    c.request_repaint();
                }
                return 1;
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, w, l) }
}
