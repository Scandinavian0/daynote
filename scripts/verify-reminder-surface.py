"""Windows-only fault injection for an ISOLATED DayNote test process.

Use a build with a separate Tauri identifier, data directory and WebView2 profile.
Never point this at a user's running installation. See docs/REMINDER-WINDOW.md.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import json
from pathlib import Path
import time

if not __debug__:
    raise SystemExit("Run without -O / PYTHONOPTIMIZE; assertions are required for fault-injection safety")

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--pid", type=int, required=True)
parser.add_argument("--expected-exe", type=Path, required=True)
parser.add_argument("--rounds", type=int, default=10)
args = parser.parse_args()
if args.rounds < 1:
    parser.error("--rounds must be at least 1")
user = c.WinDLL("user32", use_last_error=True)
kernel = c.WinDLL("kernel32", use_last_error=True)
callback = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
for name, types, result in [
    ("EnumWindows", [callback, w.LPARAM], w.BOOL),
    ("EnumChildWindows", [w.HWND, callback, w.LPARAM], w.BOOL),
    ("GetWindowThreadProcessId", [w.HWND, c.POINTER(w.DWORD)], w.DWORD),
    ("GetWindowRect", [w.HWND, c.POINTER(w.RECT)], w.BOOL),
    ("GetParent", [w.HWND], w.HWND),
    ("IsWindowVisible", [w.HWND], w.BOOL),
    ("GetClassNameW", [w.HWND, w.LPWSTR, c.c_int], c.c_int),
    ("GetWindowTextW", [w.HWND, w.LPWSTR, c.c_int], c.c_int),
    ("MapWindowPoints", [w.HWND, w.HWND, c.POINTER(w.POINT), w.UINT], c.c_int),
    ("SetWindowPos", [w.HWND, w.HWND, c.c_int, c.c_int, c.c_int, c.c_int, w.UINT], w.BOOL),
    ("GetForegroundWindow", [], w.HWND),
    ("SetThreadDpiAwarenessContext", [w.HANDLE], w.HANDLE),
]:
    fn = getattr(user, name)
    fn.argtypes, fn.restype = types, result
user.SetThreadDpiAwarenessContext(w.HANDLE(-4))
kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
kernel.OpenProcess.restype = w.HANDLE
kernel.QueryFullProcessImageNameW.argtypes = [w.HANDLE, w.DWORD, w.LPWSTR, c.POINTER(w.DWORD)]
kernel.CloseHandle.argtypes = [w.HANDLE]
process = kernel.OpenProcess(0x1000, False, args.pid)
assert process, "Target process not found"
try:
    path, length = c.create_unicode_buffer(32768), w.DWORD(32768)
    assert kernel.QueryFullProcessImageNameW(process, 0, path, c.byref(length))
    assert Path(path.value).resolve() == args.expected_exe.resolve(), "Unexpected target executable"
finally:
    kernel.CloseHandle(process)

def text(hwnd, fn):
    value = c.create_unicode_buffer(256)
    fn(hwnd, value, len(value))
    return value.value

def rect(hwnd):
    value = w.RECT()
    assert user.GetWindowRect(hwnd, c.byref(value))
    return [value.left, value.top, value.right, value.bottom]

def offset(hwnd):
    bounds = rect(hwnd)
    point = w.POINT(bounds[0], bounds[1])
    user.MapWindowPoints(None, user.GetParent(hwnd), c.byref(point), 1)
    return [point.x, point.y]

roots, surfaces = [], []
@callback
def top(hwnd, _):
    pid = w.DWORD()
    user.GetWindowThreadProcessId(hwnd, c.byref(pid))
    if pid.value == args.pid and text(hwnd, user.GetWindowTextW) == "DayNote 提醒" and user.IsWindowVisible(hwnd):
        roots.append(hwnd)
    return True

@callback
def child(hwnd, _):
    if text(hwnd, user.GetClassNameW) == "Intermediate D3D Window" and user.IsWindowVisible(hwnd):
        surfaces.append(hwnd)
    return True

user.EnumWindows(top, 0)
assert len(roots) == 1, "Expected one visible isolated reminder; create a due fixture task first"
user.EnumChildWindows(roots[0], child, 0)
assert len(surfaces) == 1, "Expected one WebView2 rendering surface"
root, surface = roots[0], surfaces[0]
assert offset(surface) == [0, 0], "Test must start with a healthy surface"
flags = 0x1 | 0x4 | 0x10 | 0x200 | 0x4000  # NOSIZE, NOZORDER, NOACTIVATE, NOOWNERZORDER, ASYNC
results = []
try:
    for iteration in range(args.rounds):
        original, focus = rect(root), user.GetForegroundWindow()
        # Reproduce the observed screen-(0,0) surface, plus offscreen on the other side.
        x, y = (-original[0], -original[1]) if iteration % 2 == 0 else (5000, 5000)
        observed = False
        for attempt in range(3):
            assert user.SetWindowPos(surface, None, x, y, 0, 0, flags)
            injection_deadline = time.monotonic() + 1
            while time.monotonic() < injection_deadline:
                if offset(surface) == [x, y]:
                    observed = True
                    break
                time.sleep(0.001)
            if observed:
                break
        assert observed, "INCONCLUSIVE: the asynchronous fault was never observed"
        deadline = time.monotonic() + 3
        while offset(surface) != [0, 0] and time.monotonic() < deadline:
            time.sleep(0.05)
        assert offset(surface) == [0, 0], "FAIL: invisible input-blocking reminder did not recover"
        assert rect(root) == original, "Recovery moved or resized the outer reminder"
        assert user.GetForegroundWindow() == focus, "Recovery stole focus (or external input changed focus)"
        results.append({"round": iteration + 1, "recovered": True})
finally:
    # Leave the owned test process usable even when the old version fails.
    user.SetWindowPos(surface, None, 0, 0, 0, 0, flags)
print(json.dumps({"passed": True, "rounds": results}))
