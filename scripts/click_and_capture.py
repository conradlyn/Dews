"""Focus the ReadYou window, click a relative point via SendInput, capture.

Usage: python click_and_capture.py <rel_x> <rel_y> <output.png>
Coordinates are physical pixels relative to the window's top-left.
"""

import ctypes
import sys
import time
from ctypes import wintypes

sys.path.insert(0, "C:/Users/lfk/WorkBuddy/2026-11-23-08-55-49/ReadYouDesktop/scripts")
import capture_window as cap

user32 = ctypes.windll.user32

# SendInput structures
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


def main():
    rel_x, rel_y, out = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
    wins = cap.find_window("ReadYou")
    if not wins:
        print("window not found")
        sys.exit(1)
    hwnd, title = wins[0]
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    x, y = rect.left + rel_x, rect.top + rel_y
    print(f"rect=({rect.left},{rect.top},{rect.right},{rect.bottom}) click=({x},{y})")
    user32.ShowWindow(hwnd, 9)
    # Windows foreground lock: a synthetic Alt keypress allows focus change.
    user32.keybd_event(0x12, 0, 0, 0)  # ALT down
    user32.SetForegroundWindow(hwnd)
    user32.BringWindowToTop(hwnd)
    user32.keybd_event(0x12, 0, 2, 0)  # ALT up
    time.sleep(0.6)
    fg = user32.GetForegroundWindow()
    print("foreground matches:", fg == hwnd)
    # Move the cursor with SendInput (absolute coords normalized to 65535).
    screen_w = user32.GetSystemMetrics(0)
    screen_h = user32.GetSystemMetrics(1)
    nx = int(x * 65535 / max(screen_w - 1, 1))
    ny = int(y * 65535 / max(screen_h - 1, 1))
    send_mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, nx, ny)
    time.sleep(0.35)
    send_mouse(MOUSEEVENTF_LEFTDOWN)
    time.sleep(0.12)
    send_mouse(MOUSEEVENTF_LEFTUP)
    time.sleep(2.0)
    cap.capture(hwnd, out)


if __name__ == "__main__":
    main()
