#!/usr/bin/env python3
"""Generate assets/brand/machine.svg: an original exploded isometric line drawing of a computer's main
parts (board, processor, memory, graphics card, M.2 drive). Stroke-only so GPUI can colour it per theme.
Prints the callout anchor points as fractions of the canvas for the app to place its labels."""
import math
C, S = math.cos(math.radians(30)), math.sin(math.radians(30))
VW, VH = 640, 430
OX, OY = 268, 214
def P(x, y, z):
    return (OX + (x - y) * C, OY + (x + y) * S - z)
def path(pts, close=True, **kw):
    d = "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in pts) + (" Z" if close else "")
    extra = "".join(f' {k.replace("_","-")}="{v}"' for k, v in kw.items())
    return f'<path d="{d}"{extra}/>'
out = []
def line(a, b, **kw): out.append(path([a, b], close=False, **kw))
def box(x, y, z, w, d, h=0, **kw):
    t = [P(x, y, z + h), P(x + w, y, z + h), P(x + w, y + d, z + h), P(x, y + d, z + h)]
    out.append(path(t, **kw))
    if h:
        out.append(path([P(x + w, y, z + h), P(x + w, y + d, z + h), P(x + w, y + d, z), P(x + w, y, z)], **kw))
        out.append(path([P(x, y + d, z + h), P(x + w, y + d, z + h), P(x + w, y + d, z), P(x, y + d, z)], **kw))
def circle(cx, cy, z, r, n=28, **kw):
    out.append(path([P(cx + r * math.cos(2 * math.pi * i / n), cy + r * math.sin(2 * math.pi * i / n), z) for i in range(n)], **kw))
def dashed(a, b): line(a, b, stroke_dasharray="2 5", opacity="0.6")

W, D = 300, 205
# --- board ---
box(0, 0, 0, W, D, 6)
box(8, 8, 6, W - 16, D - 16, 0, opacity="0.35")
# CPU socket + VRM blocks
box(62, 62, 6, 64, 64, 4); box(70, 70, 10, 48, 48, 0, opacity="0.7"); box(78, 78, 10, 32, 32, 0, opacity="0.5")
for i in range(3):
    box(20, 44 + i * 30, 6, 26, 16, 9, opacity="0.85")                # power blocks
for i in range(3):
    box(70 + i * 26, 22, 6, 18, 20, 9, opacity="0.85")
# memory slots
for x in (236, 256):
    box(x, 30, 6, 11, 128, 5)
# PCIe slots, chipset, M.2 slot, io
box(30, 150, 6, 170, 11, 5); box(30, 176, 6, 120, 9, 4)
box(236, 176, 6, 44, 22, 4, opacity="0.8")
box(18, 20, 6, 30, 100, 11, opacity="0.7")                            # I/O shield
box(150, 100, 6, 36, 24, 6)                                           # chipset
for i in range(7): line(P(30 + i * 24, 120, 6), P(30 + i * 24, 138, 6), opacity="0.5")
# --- exploded pieces ---
# CPU chip above its socket
cz = 112
for cx, cy in ((62, 62), (126, 62), (126, 126), (62, 126)):
    dashed(P(cx, cy, 10), P(cx, cy, cz))
box(62, 62, cz, 64, 64, 6); box(74, 74, cz + 6, 40, 40, 0, opacity="0.8")
for i in range(8):
    line(P(62 + 8 * i, 62, cz), P(62 + 8 * i, 58, cz), opacity="0.6")
cpu_anchor = P(94, 94, cz + 6)
# memory sticks above their slots
mz = 62
for x in (236, 256):
    for cy in (30, 158):
        dashed(P(x + 5, cy, 12), P(x + 5, cy, mz))
    box(x, 30, mz, 9, 128, 34)
    for i in range(5):
        line(P(x, 42 + i * 22, mz + 34), P(x + 9, 42 + i * 22, mz + 34), opacity="0.7")
        line(P(x + 9, 42 + i * 22, mz + 34), P(x + 9, 42 + i * 22, mz + 6), opacity="0.35")
mem_anchor = P(256 + 9, 70, mz + 22)
# graphics card, lower right, lying beside the board with three fans
gz = -46
box(40, 222, gz, 250, 62, 14)
for i, fx in enumerate((92, 165, 238)):
    circle(fx, 253, gz + 14, 26); circle(fx, 253, gz + 14, 9, 14)
    for k in range(7):
        a = k * math.pi * 2 / 7
        line(P(fx + 9 * math.cos(a), 253 + 9 * math.sin(a), gz + 14), P(fx + 24 * math.cos(a + .6), 253 + 24 * math.sin(a + .6), gz + 14), opacity="0.7")
box(6, 232, gz - 2, 36, 44, 16, opacity="0.85")                       # bracket
gpu_anchor = P(238, 253, gz + 14)
dashed(P(30, 150, 10), P(30, 222, gz + 14))
# M.2 drive, lower left
mz2 = -34
box(-70, 120, mz2, 92, 30, 3); box(-62, 125, mz2 + 3, 20, 20, 2, opacity="0.8"); box(-34, 125, mz2 + 3, 20, 20, 2, opacity="0.8")
dashed(P(22, 135, mz2 + 3), P(30, 168, 6))
m2_anchor = P(-24, 135, mz2 + 3)
import re
pts = [(float(a), float(b)) for el in out for d in re.findall(r' d="([^"]*)"', el) for a, b in re.findall(r"(-?\d+\.?\d*) (-?\d+\.?\d*)", d)]
minx, maxx = min(x for x, _ in pts), max(x for x, _ in pts)
miny, maxy = min(y for _, y in pts), max(y for _, y in pts)
M = 14
k = min((VW - 2 * M) / (maxx - minx), (VH - 2 * M) / (maxy - miny))
dx = (VW - k * (maxx - minx)) / 2 - k * minx
dy = (VH - k * (maxy - miny)) / 2 - k * miny
def T(a): return (a[0] * k + dx, a[1] * k + dy)
cpu_anchor, mem_anchor, gpu_anchor, m2_anchor = map(T, (cpu_anchor, mem_anchor, gpu_anchor, m2_anchor))
svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{VW}" height="{VH}" viewBox="0 0 {VW} {VH}" fill="none" '
       'stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" stroke-linecap="round">'
       f'<g transform="translate({dx:.2f} {dy:.2f}) scale({k:.4f})" stroke-width="{1.3 / k:.3f}">' + "".join(out) + "</g></svg>\n")
open("assets/brand/machine.svg", "w").write(svg)
xs = [p[0] for p in (cpu_anchor, mem_anchor, gpu_anchor, m2_anchor)]
print("viewBox", VW, VH)
for n, a in (("cpu", cpu_anchor), ("memory", mem_anchor), ("graphics", gpu_anchor), ("m2", m2_anchor)):
    print(f"{n:9} x={a[0]/VW:.3f} y={a[1]/VH:.3f}")
