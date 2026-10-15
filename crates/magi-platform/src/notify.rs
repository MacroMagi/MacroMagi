//! Notifications and the clipboard.
//!
//! * The sound is the system's own notification sound.
//! * On Windows the notification is a tray balloon, which Windows 10 and 11
//!   show as a regular notification under MacroMagi's name. macOS uses
//!   `osascript`, Linux `notify-send`.

use magi_core::host::{HostError, Notification};

/// Plays a sound and/or shows a desktop notification. Returns at once: the
/// notification stays up on its own.
pub fn notify(notification: &Notification) -> Result<(), HostError> {
    if notification.sound {
        ding();
    }
    if notification.popup {
        show(&notification.title, &notification.message)?;
    }
    Ok(())
}

/// The text on the clipboard; empty when it holds something else.
pub fn clipboard_text() -> Result<String, HostError> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| HostError::new(format!("the clipboard is not available: {e}")))?;
    match clipboard.get_text() {
        Ok(text) => Ok(text),
        Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
        Err(error) => Err(HostError::new(format!("could not read the clipboard: {error}"))),
    }
}

#[cfg(windows)]
fn ding() {
    use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
    // SAFETY: a plain call with a constant argument.
    unsafe { MessageBeep(MB_ICONASTERISK) };
}

#[cfg(target_os = "macos")]
fn ding() {
    let _ = std::process::Command::new("afplay").arg("/System/Library/Sounds/Glass.aiff").spawn();
}

#[cfg(all(unix, not(target_os = "macos")))]
fn ding() {
    let played = std::process::Command::new("canberra-gtk-play").args(["-i", "complete"]).spawn().is_ok()
        || std::process::Command::new("paplay")
            .arg("/usr/share/sounds/freedesktop/stereo/complete.oga")
            .spawn()
            .is_ok();
    if !played {
        use std::io::Write as _;
        let _ = std::io::stdout().write_all(b"\x07");
    }
}

#[cfg(target_os = "macos")]
fn show(title: &str, message: &str) -> Result<(), HostError> {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\"", quote(message), quote(title));
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .spawn()
        .map(drop)
        .map_err(|e| HostError::new(format!("could not show a notification: {e}")))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn show(title: &str, message: &str) -> Result<(), HostError> {
    std::process::Command::new("notify-send")
        .args(["--app-name=MacroMagi", title, message])
        .spawn()
        .map(drop)
        .map_err(|e| HostError::new(format!("could not show a notification (is notify-send installed?): {e}")))
}

#[cfg(windows)]
fn show(title: &str, message: &str) -> Result<(), HostError> {
    let (title, message) = (title.to_owned(), message.to_owned());
    std::thread::Builder::new()
        .name("magi-notify".into())
        .spawn(move || balloon(&title, &message))
        .map(drop)
        .map_err(|e| HostError::new(format!("could not show a notification: {e}")))
}

/// Shows a tray balloon, keeps the tray icon long enough for it to be seen,
/// then removes the icon again.
#[cfg(windows)]
fn balloon(title: &str, message: &str) {
    use std::ptr;

    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_INFO, NIF_TIP, NIIF_NOSOUND, NIIF_USER, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE, IDI_APPLICATION, LoadIconW,
    };

    fn copy(into: &mut [u16], text: &str) {
        let wide: Vec<u16> = text.encode_utf16().take(into.len() - 1).collect();
        into[..wide.len()].copy_from_slice(&wide);
        into[wide.len()] = 0;
    }

    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    // SAFETY: plain Win32 calls with valid arguments; the structure is fully
    // initialised and sized, and the window lives until the icon is removed.
    unsafe {
        let module = GetModuleHandleW(ptr::null());
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            ptr::null_mut(),
            module,
            ptr::null(),
        );
        if window.is_null() {
            return;
        }
        // The executable's own icon (resource 1), or the generic one.
        let mut icon = LoadIconW(module, std::ptr::without_provenance(1));
        if icon.is_null() {
            icon = LoadIconW(ptr::null_mut(), IDI_APPLICATION);
        }
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = window;
        data.uID = 1;
        data.uFlags = NIF_ICON | NIF_TIP | NIF_INFO;
        data.hIcon = icon;
        data.hBalloonIcon = icon;
        data.dwInfoFlags = NIIF_USER | NIIF_NOSOUND;
        copy(&mut data.szTip, "MacroMagi");
        copy(&mut data.szInfoTitle, title);
        copy(&mut data.szInfo, if message.is_empty() { " " } else { message });
        if Shell_NotifyIconW(NIM_ADD, &data) != 0 {
            std::thread::sleep(std::time::Duration::from_secs(8));
            Shell_NotifyIconW(NIM_DELETE, &data);
        }
        DestroyWindow(window);
    }
}
