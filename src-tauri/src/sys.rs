//! The few Win32 calls Tauri does not wrap: showing a window without activating it,
//! the Windows build number, and the system notification sound.
use std::ffi::c_void;

type Hwnd = *mut c_void;

#[cfg(windows)]
mod ffi {
    use super::Hwnd;

    #[repr(C)]
    pub struct OsVersionInfoW {
        pub size: u32,
        pub major: u32,
        pub minor: u32,
        pub build: u32,
        pub platform: u32,
        pub csd: [u16; 128],
    }

    #[link(name = "user32")]
    extern "system" {
        pub fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        pub fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
        pub fn MessageBeep(kind: u32) -> i32;
    }

    #[link(name = "winmm")]
    extern "system" {
        pub fn PlaySoundW(sound: *const u16, module: *mut core::ffi::c_void, flags: u32) -> i32;
    }

    #[link(name = "ntdll")]
    extern "system" {
        pub fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
    }

    #[link(name = "dwmapi")]
    extern "system" {
        pub fn DwmSetWindowAttribute(hwnd: Hwnd, attr: u32, value: *const core::ffi::c_void, size: u32) -> i32;
    }
}

/// Make DWM draw this window's backdrop and frame in dark mode whatever the system theme
/// is. Without it the acrylic backdrop (and its solid fallback while the window is
/// inactive) is light grey on a light-themed Windows, which washes the dark panel out.
#[cfg(windows)]
pub fn use_dark_frame(hwnd: Hwnd) {
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    let on: i32 = 1;
    // SAFETY: plain Win32 call; `on` outlives the call and the size matches its type.
    unsafe {
        ffi::DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&on as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg(not(windows))]
pub fn use_dark_frame(_hwnd: Hwnd) {}

/// Ask Windows 11 for rounded window corners (a borderless window without the system
/// shadow gets square ones by default). Ignored on Windows 10.
#[cfg(windows)]
pub fn round_corners(hwnd: Hwnd) {
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: i32 = 2;
    // SAFETY: plain Win32 call; the value outlives the call and the size matches its type.
    unsafe {
        ffi::DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&DWMWCP_ROUND as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg(not(windows))]
pub fn round_corners(_hwnd: Hwnd) {}

/// Write `HKCU\...\Run\<name> = <command>`. The autostart plugin stores the exe path
/// unquoted, which is ambiguous when the path contains spaces; this rewrites the entry
/// with a properly quoted command line.
#[cfg(windows)]
pub fn set_run_entry(name: &str, command: &str) -> bool {
    #[link(name = "advapi32")]
    extern "system" {
        fn RegSetKeyValueW(
            key: isize,
            subkey: *const u16,
            value: *const u16,
            kind: u32,
            data: *const c_void,
            size: u32,
        ) -> i32;
    }
    // HKEY_CURRENT_USER is the sign-extended constant 0x80000001.
    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const REG_SZ: u32 = 1;
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let subkey = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let value = wide(name);
    let data = wide(command);
    // SAFETY: all three buffers are NUL-terminated UTF-16 that outlive the call, and the
    // size passed is the byte length of `data` including its terminator.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        ) == 0
    }
}

#[cfg(not(windows))]
pub fn set_run_entry(_name: &str, _command: &str) -> bool {
    false
}

/// Acrylic blur with our own tint, through the accent-policy call that the taskbar and
/// Start menu use. Unlike the DWM "system backdrop", it keeps blurring while the window
/// is inactive and its colour does not depend on the system theme. `tint` is (r, g, b, a).
/// The function is not in user32's import library, so it is looked up at run time.
#[cfg(windows)]
pub fn apply_tinted_acrylic(hwnd: Hwnd, tint: (u8, u8, u8, u8)) -> bool {
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attribute: u32,
        data: *mut c_void,
        size: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }
    const WCA_ACCENT_POLICY: u32 = 19;
    const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
    type SetAttr = unsafe extern "system" fn(Hwnd, *mut CompositionData) -> i32;

    let (r, g, b, a) = tint;
    let mut policy = AccentPolicy {
        state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
        flags: 2,
        gradient: (u32::from(a) << 24) | (u32::from(b) << 16) | (u32::from(g) << 8) | u32::from(r),
        animation: 0,
    };
    let mut data = CompositionData {
        attribute: WCA_ACCENT_POLICY,
        data: (&mut policy as *mut AccentPolicy).cast(),
        size: std::mem::size_of::<AccentPolicy>(),
    };
    // SAFETY: both names are C string literals; the looked-up symbol has the signature
    // declared above on every Windows 10/11 build; `data` and `policy` outlive the call.
    unsafe {
        let user32 = LoadLibraryA(c"user32.dll".as_ptr().cast());
        if user32.is_null() {
            return false;
        }
        let f = GetProcAddress(user32, c"SetWindowCompositionAttribute".as_ptr().cast());
        if f.is_null() {
            return false;
        }
        let set: SetAttr = std::mem::transmute::<*mut c_void, SetAttr>(f);
        set(hwnd, &mut data) != 0
    }
}

#[cfg(not(windows))]
pub fn apply_tinted_acrylic(_hwnd: Hwnd, _tint: (u8, u8, u8, u8)) -> bool {
    false
}

const HWND_TOPMOST: isize = -1;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_SHOWWINDOW: u32 = 0x0040;
const SWP_NOOWNERZORDER: u32 = 0x0200;
const SW_HIDE: i32 = 0;

/// Place a window topmost at the given physical rect and show it without taking focus.
#[cfg(windows)]
pub fn show_no_activate(hwnd: Hwnd, x: i32, y: i32, w: i32, h: i32) {
    // SAFETY: plain Win32 call on a window handle owned by this process.
    unsafe {
        ffi::SetWindowPos(
            hwnd,
            HWND_TOPMOST as Hwnd,
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_NOOWNERZORDER,
        );
    }
}

#[cfg(windows)]
pub fn hide(hwnd: Hwnd) {
    // SAFETY: plain Win32 call on a window handle owned by this process.
    unsafe {
        ffi::ShowWindow(hwnd, SW_HIDE);
    }
}

/// Windows build number (e.g. 22631), 0 if unknown.
#[cfg(windows)]
pub fn windows_build() -> u32 {
    let mut info = ffi::OsVersionInfoW {
        size: std::mem::size_of::<ffi::OsVersionInfoW>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        csd: [0; 128],
    };
    // SAFETY: `info` is a correctly sized OSVERSIONINFOW owned by this frame.
    let status = unsafe { ffi::RtlGetVersion(&mut info) };
    if status == 0 {
        info.build
    } else {
        0
    }
}

/// Play the Windows notification sound asynchronously (never blocks).
#[cfg(windows)]
pub fn play_notification() {
    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_ALIAS: u32 = 0x0001_0000;
    const SND_SYSTEM: u32 = 0x0020_0000;
    const MB_ICONASTERISK: u32 = 0x40;
    let alias: Vec<u16> = "Notification.Default\0".encode_utf16().collect();
    // SAFETY: `alias` is a NUL-terminated UTF-16 string that outlives the call; with
    // SND_ASYNC PlaySound copies what it needs and returns immediately.
    unsafe {
        let ok = ffi::PlaySoundW(
            alias.as_ptr(),
            std::ptr::null_mut(),
            SND_ALIAS | SND_ASYNC | SND_NODEFAULT | SND_SYSTEM,
        );
        if ok == 0 {
            ffi::MessageBeep(MB_ICONASTERISK);
        }
    }
}

#[cfg(not(windows))]
pub fn show_no_activate(_hwnd: Hwnd, _x: i32, _y: i32, _w: i32, _h: i32) {}
#[cfg(not(windows))]
pub fn hide(_hwnd: Hwnd) {}
#[cfg(not(windows))]
pub fn windows_build() -> u32 {
    0
}
#[cfg(not(windows))]
pub fn play_notification() {}
