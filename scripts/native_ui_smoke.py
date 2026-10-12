# /// script
# requires-python = ">=3.12"
# dependencies = ["pillow==11.3.0"]
# ///
"""Drive and capture only the running Greign X11 window for desktop smoke tests."""

import ctypes as C
import json
import subprocess
import sys
import time
from pathlib import Path
from PIL import Image

x = C.CDLL("libX11.so.6")
t = C.CDLL("libXtst.so.6")
x.XOpenDisplay.restype = C.c_void_p
x.XOpenDisplay.argtypes = [C.c_char_p]
display = x.XOpenDisplay(None)
if not display:
    raise SystemExit("X11 display unavailable")
rows = subprocess.check_output(["wmctrl", "-lpG"], text=True).splitlines()
matches = [r.split(None, 8) for r in rows if "Greign — Film workspace" in r]
if len(matches) != 1:
    raise SystemExit(f"Expected exactly one Greign window, found {len(matches)}")
row = matches[0]
window = int(row[0], 16)
x.XFlush.argtypes = [C.c_void_p]
x.XSync.argtypes = [C.c_void_p,C.c_int]
x.XWarpPointer.argtypes = [C.c_void_p,C.c_ulong,C.c_ulong,C.c_int,C.c_int,C.c_uint,C.c_uint,C.c_int,C.c_int]
x.XRaiseWindow.argtypes = [C.c_void_p, C.c_ulong]
x.XSetInputFocus.argtypes = [C.c_void_p, C.c_ulong, C.c_int, C.c_ulong]
x.XTranslateCoordinates.argtypes = [C.c_void_p, C.c_ulong, C.c_ulong, C.c_int, C.c_int, C.POINTER(C.c_int), C.POINTER(C.c_int), C.POINTER(C.c_ulong)]
x.XDefaultRootWindow.argtypes = [C.c_void_p]
x.XDefaultRootWindow.restype = C.c_ulong
left, top, child = C.c_int(), C.c_int(), C.c_ulong()
x.XTranslateCoordinates(display, window, x.XDefaultRootWindow(display), 0, 0, C.byref(left), C.byref(top), C.byref(child))
t.XTestFakeMotionEvent.argtypes = [C.c_void_p, C.c_int, C.c_int, C.c_int, C.c_ulong]
t.XTestFakeButtonEvent.argtypes = [C.c_void_p, C.c_uint, C.c_int, C.c_ulong]
t.XTestFakeKeyEvent.argtypes = [C.c_void_p, C.c_uint, C.c_int, C.c_ulong]
x.XStringToKeysym.argtypes = [C.c_char_p]
x.XStringToKeysym.restype = C.c_ulong
x.XKeysymToKeycode.argtypes = [C.c_void_p, C.c_ulong]
x.XKeysymToKeycode.restype = C.c_uint

def move(px, py):
    x.XWarpPointer(display, 0, window, 0, 0, 0, 0, int(px), int(py))
    x.XFlush(display)


command = sys.argv[1]
if command == "pointer":
    if len(sys.argv)>2:
        move(*map(int,sys.argv[2:4]));x.XSync(display,0);time.sleep(.2)
    root_return, child_return=C.c_ulong(),C.c_ulong()
    rx,ry,wx,wy,mask=C.c_int(),C.c_int(),C.c_int(),C.c_int(),C.c_uint()
    x.XQueryPointer.argtypes=[C.c_void_p,C.c_ulong,C.POINTER(C.c_ulong),C.POINTER(C.c_ulong),C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(C.c_int),C.POINTER(C.c_uint)]
    x.XQueryPointer(display,window,C.byref(root_return),C.byref(child_return),C.byref(rx),C.byref(ry),C.byref(wx),C.byref(wy),C.byref(mask))
    print(json.dumps({"root":[rx.value,ry.value],"window":[wx.value,wy.value],"mask":mask.value,"origin":[left.value,top.value]}))
elif command == "close":
    subprocess.run(["wmctrl","-ic",row[0]],check=True)
elif command == "info":
    print(json.dumps({"window": row[0], "pid": row[2], "x": left.value, "y": top.value, "width": row[5], "height": row[6]}))
elif command in ("click", "drag", "key"):
    x.XRaiseWindow(display, window)
    x.XSetInputFocus(display, window, 2, 0)
    x.XSync(display,0)
    time.sleep(.15)
    if command == "key":
        codes = [x.XKeysymToKeycode(display, x.XStringToKeysym(k.encode())) for k in sys.argv[2].split("+")]
        for code in codes:
            t.XTestFakeKeyEvent(display, code, 1, 0)
        for code in reversed(codes):
            t.XTestFakeKeyEvent(display, code, 0, 0)
    else:
        px, py = map(int, sys.argv[2:4])
        move(px, py)
        x.XSync(display,0)
        time.sleep(.15)
        t.XTestFakeButtonEvent(display, 1, 1, 0)
        x.XFlush(display)
        time.sleep(.08)
        if command == "drag":
            dx, dy = map(int, sys.argv[4:6])
            for i in range(1, 13):
                move(px + (dx-px)*i/12, py + (dy-py)*i/12)
                time.sleep(.02)
        t.XTestFakeButtonEvent(display, 1, 0, 0)
    x.XFlush(display)
    x.XSync(display,0)
    time.sleep(.2)
elif command == "capture":
    class XImage(C.Structure):
        _fields_ = [("width", C.c_int), ("height", C.c_int), ("xoffset", C.c_int), ("format", C.c_int), ("data", C.c_void_p),
                    ("byte_order", C.c_int), ("bitmap_unit", C.c_int), ("bitmap_bit_order", C.c_int), ("bitmap_pad", C.c_int),
                    ("depth", C.c_int), ("bytes_per_line", C.c_int), ("bits_per_pixel", C.c_int),
                    ("red_mask", C.c_ulong), ("green_mask", C.c_ulong), ("blue_mask", C.c_ulong)]
    x.XGetImage.argtypes = [C.c_void_p, C.c_ulong, C.c_int, C.c_int, C.c_uint, C.c_uint, C.c_ulong, C.c_int]
    x.XGetImage.restype = C.POINTER(XImage)
    x.XDestroyImage.argtypes = [C.POINTER(XImage)]
    width, height = int(row[5]), int(row[6])
    captured = x.XGetImage(display, window, 0, 0, width, height, C.c_ulong(-1).value, 2)
    if not captured:
        raise SystemExit("Window capture failed")
    value = captured.contents
    assert value.bits_per_pixel == 32 and value.byte_order == 0 and value.red_mask == 0xff0000
    data = C.string_at(value.data, value.bytes_per_line * value.height)
    image = Image.frombytes("RGB", (width, height), data, "raw", "BGRX", value.bytes_per_line)
    destination = Path(sys.argv[2])
    destination.parent.mkdir(parents=True, exist_ok=True)
    image.save(destination)
    x.XDestroyImage(captured)
    print(destination)
else:
    raise SystemExit("Use info, click X Y, drag X Y X Y, key NAME, or capture FILE")
