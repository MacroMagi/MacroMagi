//! Windows, processes and the desktop.
//!
//! * which program has keyboard focus (for process rules);
//! * finding a window and bringing it to the front (Switch to Window);
//! * the list of running programs (for the process rules editor);
//! * creating a desktop shortcut, and giving memory back to the system.
//!
//! Windows is fully supported. Elsewhere the process list works, the rest
//! reports that it is unavailable.

#[cfg(not(windows))]
use magi_core::host::{HostError, WindowInfo, WindowQuery};

/// A running program, for the process rules editor.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessEntry {
    /// Executable name, e.g. `game.exe`.
    pub name: String,
    /// Whether it has a visible window – the ones people usually mean.
    pub has_window: bool,
}

#[cfg(windows)]
pub use self::windows::{focus_window, foreground_process, running_processes, trim_memory};

#[cfg(not(windows))]
pub fn foreground_process() -> Option<String> {
    None
}

#[cfg(not(windows))]
pub fn focus_window(_query: &WindowQuery) -> Result<Option<WindowInfo>, HostError> {
    Err(HostError::new("switching windows is not supported on this system yet"))
}

#[cfg(not(windows))]
pub fn trim_memory() {}

/// Every running program, programs with windows first, each name once.
#[cfg(not(windows))]
pub fn running_processes() -> Vec<ProcessEntry> {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        // Linux: /proc/<pid>/comm holds the program name.
        for entry in entries.flatten() {
            if let Ok(name) = std::fs::read_to_string(entry.path().join("comm")) {
                names.push(name.trim().to_owned());
            }
        }
    } else if let Ok(output) = std::process::Command::new("ps").args(["-axco", "command"]).output() {
        // macOS and other Unixes.
        names.extend(String::from_utf8_lossy(&output.stdout).lines().skip(1).map(|l| l.trim().to_owned()));
    }
    names.retain(|n| !n.is_empty());
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names.into_iter().map(|name| ProcessEntry { name, has_window: false }).collect()
}

/// Icon files for a desktop shortcut: Windows shortcuts take an `.ico`,
/// Linux desktop entries a `.png`.
pub struct ShortcutIcon<'a> {
    pub ico: &'a std::path::Path,
    pub png: &'a std::path::Path,
}

/// Puts a shortcut to this executable on the user's desktop. This is the one
/// thing MacroMagi writes outside its own folder, and only when asked.
pub fn create_desktop_shortcut(icon: Option<ShortcutIcon<'_>>) -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let folder = exe.parent().map(std::path::Path::to_path_buf).unwrap_or_default();

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let quote = |p: &std::path::Path| p.display().to_string().replace('\'', "''");
        let script = format!(
            "$desktop = [Environment]::GetFolderPath('Desktop'); \
             $link = Join-Path $desktop 'MacroMagi.lnk'; \
             $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($link); \
             $shortcut.TargetPath = '{}'; $shortcut.WorkingDirectory = '{}'; \
             $shortcut.IconLocation = '{},0'; $shortcut.Save(); Write-Output $link",
            quote(&exe),
            quote(&folder),
            quote(icon.as_ref().map_or(exe.as_path(), |icon| icon.ico))
        );
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("could not run PowerShell: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        Ok(std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or("no home folder")?;
        let link = std::path::PathBuf::from(home).join("Desktop").join("MacroMagi");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&exe, &link).map_err(|e| e.to_string())?;
        let _ = (folder, icon);
        Ok(link)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let home = std::env::var_os("HOME").ok_or("no home folder")?;
        let desktop = std::process::Command::new("xdg-user-dir")
            .arg("DESKTOP")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|d| !d.is_empty())
            .map_or_else(|| std::path::PathBuf::from(&home).join("Desktop"), std::path::PathBuf::from);
        std::fs::create_dir_all(&desktop).map_err(|e| e.to_string())?;
        let link = desktop.join("MacroMagi.desktop");
        let icon_line = icon.map(|icon| format!("Icon={}\n", icon.png.display())).unwrap_or_default();
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=MacroMagi\nExec=\"{}\"\nPath={}\n{icon_line}Terminal=false\n",
            exe.display(),
            folder.display()
        );
        std::fs::write(&link, entry).map_err(|e| e.to_string())?;
        let _ = std::fs::set_permissions(&link, std::fs::Permissions::from_mode(0o755));
        Ok(link)
    }
}

#[cfg(windows)]
mod windows {
    use std::ptr;

    use magi_core::host::{HostError, WindowInfo, WindowQuery};
    use magi_core::nodes::window::{ScreenKind, WindowMatch};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcess, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, SetProcessWorkingSetSize,
    };
    use windows_sys::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GW_OWNER, GWL_STYLE, GetForegroundWindow, GetWindow, GetWindowLongW,
        GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SW_RESTORE,
        SetForegroundWindow, ShowWindow, WS_CAPTION,
    };

    use super::ProcessEntry;

    fn process_name(pid: u32) -> Option<String> {
        // SAFETY: plain Win32 calls; the handle is closed before returning.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = [0u16; 1024];
            let mut size = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buffer[..size as usize]);
            path.rsplit(['\\', '/']).next().map(str::to_owned)
        }
    }

    fn window_pid(hwnd: HWND) -> u32 {
        let mut pid = 0;
        // SAFETY: a plain call with a valid out pointer.
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        pid
    }

    fn window_title(hwnd: HWND) -> String {
        let mut buffer = [0u16; 512];
        // SAFETY: the buffer length is passed.
        let length = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..length.max(0) as usize])
    }

    pub fn foreground_process() -> Option<String> {
        // SAFETY: a plain call.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return None;
        }
        process_name(window_pid(hwnd))
    }

    /// Top-level windows a person would think of as "a window": visible, titled, not owned.
    fn app_windows() -> Vec<HWND> {
        unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> i32 {
            // SAFETY: `data` is the Vec passed below, alive for the whole enumeration.
            let windows = unsafe { &mut *(data as *mut Vec<HWND>) };
            // SAFETY: plain queries on a window handle supplied by the system.
            let ordinary = unsafe {
                IsWindowVisible(hwnd) != 0
                    && GetWindow(hwnd, GW_OWNER).is_null()
                    && GetWindowTextW(hwnd, ptr::null_mut(), 0) >= 0
            };
            if ordinary && !window_title(hwnd).trim().is_empty() {
                windows.push(hwnd);
            }
            1
        }
        let mut windows: Vec<HWND> = Vec::new();
        // SAFETY: the callback only touches the Vec it is given.
        unsafe { EnumWindows(Some(collect), (&raw mut windows) as LPARAM) };
        windows
    }

    fn detect_kind(hwnd: HWND) -> ScreenKind {
        // SAFETY: plain queries with valid out pointers.
        unsafe {
            let mut rect: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut rect);
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(monitor, &mut info);
            let screen = info.rcMonitor;
            let covers = rect.left <= screen.left
                && rect.top <= screen.top
                && rect.right >= screen.right
                && rect.bottom >= screen.bottom;
            let framed = (GetWindowLongW(hwnd, GWL_STYLE) as u32) & WS_CAPTION == WS_CAPTION;
            if !covers || framed {
                return ScreenKind::Windowed;
            }
            let mut state = 0;
            let exclusive = SHQueryUserNotificationState(&mut state) == 0
                && state == QUNS_RUNNING_D3D_FULL_SCREEN
                && GetForegroundWindow() == hwnd;
            if exclusive { ScreenKind::Exclusive } else { ScreenKind::Borderless }
        }
    }

    pub fn focus_window(query: &WindowQuery) -> Result<Option<WindowInfo>, HostError> {
        let needle = query.text.to_lowercase();
        let found = app_windows().into_iter().find_map(|hwnd| {
            let title = window_title(hwnd);
            let process = process_name(window_pid(hwnd)).unwrap_or_default();
            let haystack = match query.by {
                WindowMatch::Title => title.to_lowercase(),
                WindowMatch::Process => process.to_lowercase(),
            };
            haystack.contains(&needle).then_some((hwnd, title, process))
        });
        let Some((hwnd, title, process)) = found else { return Ok(None) };

        // SAFETY: plain window management calls on a live handle. Attaching to
        // the foreground thread's input is the documented way to be allowed to
        // change the foreground window; it is undone straight away.
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            let foreground = GetForegroundWindow();
            let theirs = GetWindowThreadProcessId(foreground, ptr::null_mut());
            let ours = GetCurrentThreadId();
            let attached = theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, 1) != 0;
            BringWindowToTop(hwnd);
            SetForegroundWindow(hwnd);
            if attached {
                AttachThreadInput(ours, theirs, 0);
            }
        }
        // SAFETY: a plain call.
        let in_front = unsafe { GetForegroundWindow() } == hwnd;
        Ok(Some(WindowInfo { title, process, kind: detect_kind(hwnd), foreground: in_front }))
    }

    pub fn running_processes() -> Vec<ProcessEntry> {
        let with_windows: Vec<String> =
            app_windows().into_iter().filter_map(|hwnd| process_name(window_pid(hwnd))).collect();
        let mut names: Vec<String> = Vec::new();
        // SAFETY: a standard Toolhelp snapshot walk; the handle is closed at the end.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if !snapshot.is_null() && snapshot as isize != -1 {
                let mut entry: PROCESSENTRY32W = std::mem::zeroed();
                entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
                let mut more = Process32FirstW(snapshot, &mut entry) != 0;
                while more {
                    let length = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                    names.push(String::from_utf16_lossy(&entry.szExeFile[..length]));
                    more = Process32NextW(snapshot, &mut entry) != 0;
                }
                CloseHandle(snapshot);
            }
        }
        names.retain(|n| !n.is_empty() && !n.starts_with('[') && n != "System" && n != "Registry");
        let mut entries: Vec<ProcessEntry> = names
            .into_iter()
            .map(|name| {
                let has_window = with_windows.iter().any(|w| w.eq_ignore_ascii_case(&name));
                ProcessEntry { name, has_window }
            })
            .collect();
        entries.sort_by(|a, b| b.has_window.cmp(&a.has_window).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        entries.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
        entries
    }

    /// Hands memory MacroMagi is not using back to the system. Pages that
    /// are needed again come back on their own; nothing is lost.
    pub fn trim_memory() {
        // SAFETY: (-1, -1) is the documented "trim the working set" request.
        unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
    }
}
