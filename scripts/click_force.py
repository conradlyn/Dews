"""Force-focus the ReadYou window, click a relative point, capture over time.

Usage: python click_force.py <rel_x> <rel_y> <output_prefix>
Coordinates are physical pixels relative to the window's top-left.
"""

import ctypes
import sys
import time
from ctypes import wintypes

sys.path.insert(0, "C:/Users/lfk/WorkBuddy/2026-11-23-08-55-49/ReadYouDesktop/scripts")
import capture_window as cap

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

INPUT_MOUSE = 0
MOUSEEVENTF_MOVE = 0x0001
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
MOUSEEVENTF_ABSOLUTE = 0x8000


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wintypes.LONG),
        ("dy", wintypes.LONG),
        ("mouseData", wintypes.DWORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.POINTER(wintypes.ULONG)),
    ]


class INPUT(ctypes.Structure):
    class _U(ctypes.Union):
        _fields_ = [("mi", MOUSEINPUT)]

    _fields_ = [("type", wintypes.DWORD), ("u", _U)]


def send_mouse(flags, dx=0, dy=0):
    inp = INPUT()
    inp.type = INPUT_MOUSE
    inp.u.mi = MOUSEINPUT(dx, dy, 0, flags, 0, None)
    user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))


def force_foreground(hwnd):
    for _ in range(6):
        if user32.GetForegroundWindow() == hwnd:
            return True
        fg = user32.GetForegroundWindow()
        fg_tid = user32.GetWindowThreadProcessId(fg, None) if fg else 0
        cur_tid = kernel32.GetCurrentThreadId()
        attached = bool(fg_tid and fg_tid != cur_tid)
        if attached:
            user32.AttachThreadInput(cur_tid, fg_tid, True)
        user32.keybd_event(0x12, 0, 0, 0)
        user32.ShowWindow(hwnd, 9)
        user32.SetForegroundWindow(hwnd)
        user32.BringWindowToTop(hwnd)
        user32.keybd_event(0x12, 0, 2, 0)
        if attached:
            user32.AttachThreadInput(cur_tid, fg_tid, False)
        time.sleep(0.45)
    return user32.GetForegroundWindow() == hwnd


def main():
    rel_x, rel_y, prefix = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
    wins = cap.find_window("ReadYou")
    if not wins:
        print("window not found")
        sys.exit(1)
    hwnd = wins[0][0]
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    x, y = rect.left + rel_x, rect.top + rel_y
    print(f"rect=({rect.left},{rect.top},{rect.right},{rect.bottom}) click=({x},{y})")
    if not force_foreground(hwnd):
        print("could not focus window")
        sys.exit(2)
    print("foreground ok")

    screen_w = user32.GetSystemMetrics(0)
    screen_h = user32.GetSystemMetrics(1)
    nx = int(x * 65535 / max(screen_w - 1, 1))
    ny = int(y * 65535 / max(screen_h - 1, 1))
    send_mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, nx, ny)
    time.sleep(0.3)
    send_mouse(MOUSEEVENTF_LEFTDOWN)
    time.sleep(0.12)
    send_mouse(MOUSEEVENTF_LEFTUP)
    print("clicked")

    previous = 0
    for index, delay in enumerate([1, 3, 8, 15, 30, 45]):
        time.sleep(delay - previous)
        previous = delay
        out = f"{prefix}_{index}.png"
        cap.capture(hwnd, out)
        print(f"captured {out} at +{delay}s")


if __name__ == "__main__":
    main()
