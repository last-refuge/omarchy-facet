#!/usr/bin/env python3
"""Generate assets/brand/machine.svg: an original exploded isometric line drawing of a laptop's layers
(base, battery, board with chips, keyboard deck, display). Stroke-only so GPUI can colour it per theme."""
import math
C, S = math.cos(math.radians(30)), math.sin(math.radians(30))
ORIGIN = (250, 440)
def P(x, y, z):
    return (ORIGIN[0] + (x - y) * C, ORIGIN[1] + (x + y) * S - z)
def poly(pts, close=True, **kw):
    d = "M" + " L".join(f"{x:.1f} {y:.1f}" for x, y in pts) + (" Z" if close else "")
    extra = "".join(f' {k.replace("_","-")}="{v}"' for k, v in kw.items())
    return f'<path d="{d}"{extra}/>'
def rect3(x, y, z, w, d, h=0):
    top = [P(x, y, z + h), P(x + w, y, z + h), P(x + w, y + d, z + h), P(x, y + d, z + h)]
    out = [poly(top)]
    if h:
        out += [poly([P(x + w, y, z + h), P(x + w, y + d, z + h), P(x + w, y + d, z), P(x + w, y, z)]),
                poly([P(x, y + d, z + h), P(x + w, y + d, z + h), P(x + w, y + d, z), P(x, y + d, z)])]
    return out
parts = []
W, D = 190, 130
# connectors (dashed) between layers at the four corners, drawn first
for cx, cy in [(0, 0), (W, 0), (W, D), (0, D)]:
    a, b = P(cx, cy, 0), P(cx, cy, 214)
    parts.append(poly([a, b], close=False, stroke_dasharray="2 5", opacity="0.55"))
# layer 0: base
parts += rect3(0, 0, 0, W, D, 7)
# layer 1: battery + storage
z = 72
parts += rect3(12, 12, z, W - 24, D - 24, 0)
parts += rect3(22, 22, z, 110, 46, 9)
for i in range(1, 4):
    parts.append(poly([P(22 + i * 27.5, 22, z + 9), P(22 + i * 27.5, 68, z + 9)], close=False, opacity="0.6"))
parts += rect3(150, 30, z, 24, 70, 5)
# layer 2: board with chips
z = 138
parts += rect3(6, 6, z, W - 12, D - 12, 0)
parts += rect3(26, 30, z, 40, 40, 8)       # CPU
parts += rect3(34, 38, z + 8, 24, 24, 0)
parts += rect3(86, 28, z, 32, 32, 6)       # GPU
parts += rect3(130, 22, z, 12, 56, 7)      # RAM
parts += rect3(148, 22, z, 12, 56, 7)
parts += rect3(30, 92, z, 60, 14, 4)       # M.2
for i in range(6):
    parts.append(poly([P(100 + i * 12, 96, z), P(100 + i * 12, 118, z)], close=False, opacity="0.5"))
# layer 3: keyboard deck with key grid
z = 206
parts += rect3(0, 0, z, W, D, 5)
for r in range(3):
    for c in range(9):
        parts += rect3(12 + c * 19, 14 + r * 16, z + 5, 15, 11, 0)
parts += rect3(50, 68, z + 5, 90, 12, 0)    # space bar
parts += rect3(68, 90, z + 5, 54, 30, 0)    # trackpad
# display: standing panel behind, hinged at the back edge, raised above the stack
z = 286
panel = [P(0, 0, z), P(W, 0, z), P(W, 0, z + 118), P(0, 0, z + 118)]
parts.append(poly(panel))
inner = [P(8, 0, z + 8), P(W - 8, 0, z + 8), P(W - 8, 0, z + 110), P(8, 0, z + 110)]
parts.append(poly(inner, opacity="0.6"))
parts.append(poly([P(W / 2 - 10, 0, z + 4), P(W / 2 + 10, 0, z + 4)], close=False))
svg = ('<svg xmlns="http://www.w3.org/2000/svg" width="500" height="640" viewBox="0 0 500 640" fill="none" '
       'stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round">'
       + "".join(parts) + "</svg>\n")
open("assets/brand/machine.svg", "w").write(svg)
print("wrote assets/brand/machine.svg", len(svg), "bytes")
