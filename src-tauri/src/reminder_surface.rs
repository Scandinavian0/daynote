//! Recover the observed WebView2 child-surface displacement without showing a hidden
//! reminder or changing its focus, size or z-order. Windows/Chromium-specific fallback:
//! an unrecognised child hierarchy is left alone.

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    type Hwnd = *mut c_void;

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    extern "system" {
        fn EnumChildWindows(
            hwnd: Hwnd,
            callback: unsafe extern "system" fn(Hwnd, isize) -> i32,
            data: isize,
        ) -> i32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn IsChild(parent: Hwnd, child: Hwnd) -> i32;
        fn GetParent(hwnd: Hwnd) -> Hwnd;
        fn GetClassNameW(hwnd: Hwnd, name: *mut u16, size: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn ScreenToClient(hwnd: Hwnd, point: *mut Point) -> i32;
        fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, w: i32, h: i32, flags: u32)
            -> i32;
    }

    unsafe fn class_is(hwnd: Hwnd, expected: &str) -> bool {
        let mut name = [0u16; 128];
        let len = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32);
        len > 0
            && name[..len as usize]
                .iter()
                .copied()
                .eq(expected.encode_utf16())
    }

    unsafe extern "system" fn inspect(surface: Hwnd, root: isize) -> i32 {
        let root = root as Hwnd;
        let parent = GetParent(surface);
        if IsWindowVisible(root) == 0
            || IsWindowVisible(surface) == 0
            || IsChild(root, surface) == 0
            || IsChild(root, parent) == 0
            || !class_is(surface, "Intermediate D3D Window")
            || !class_is(parent, "Chrome_WidgetWin_1")
        {
            return 1;
        }
        let (mut bounds, mut client) = (Rect::default(), Rect::default());
        if GetWindowRect(surface, &mut bounds) == 0 || GetClientRect(parent, &mut client) == 0 {
            return 1;
        }
        let mut origin = Point {
            x: bounds.left,
            y: bounds.top,
        };
        if ScreenToClient(parent, &mut origin) == 0 {
            return 1;
        }
        let (width, height) = (
            i64::from(bounds.right) - i64::from(bounds.left),
            i64::from(bounds.bottom) - i64::from(bounds.top),
        );
        let (pw, ph) = (i64::from(client.right), i64::from(client.bottom));
        let (x, y) = (i64::from(origin.x), i64::from(origin.y));
        // Backing surfaces may be larger than the viewport after resize. Do not
        // require equal sizes, and never reposition a surface that still intersects it.
        let displaced = pw > 0
            && ph > 0
            && width >= pw
            && height >= ph
            && (x >= pw || y >= ph || x + width <= 0 || y + height <= 0);
        if displaced
            && IsWindowVisible(root) != 0
            && GetParent(surface) == parent
            && IsChild(root, surface) != 0
        {
            const FLAGS: u32 = 0x0001 | 0x0004 | 0x0010 | 0x0200 | 0x4000;
            // NOSIZE | NOZORDER | NOACTIVATE | NOOWNERZORDER | ASYNCWINDOWPOS.
            // The GPU thread owns this child. Queue the move without blocking our UI.
            // No SHOWWINDOW: dismissal must remain effective even during recovery.
            SetWindowPos(surface, std::ptr::null_mut(), 0, 0, 0, 0, FLAGS);
        }
        1
    }

    pub fn repair(root: Hwnd) {
        // SAFETY: Win32 validates handles; buffers/callback data live through the
        // synchronous enumeration. Only descendants of our visible root are touched.
        unsafe {
            let mut pid = 0;
            GetWindowThreadProcessId(root, &mut pid);
            if pid == std::process::id() && IsWindowVisible(root) != 0 {
                EnumChildWindows(root, inspect, root as isize);
            }
        }
    }
}

pub fn repair(root: *mut std::ffi::c_void) {
    #[cfg(windows)]
    windows::repair(root);
    #[cfg(not(windows))]
    let _ = root;
}
