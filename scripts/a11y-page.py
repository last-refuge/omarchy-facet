#!/usr/bin/env python3
"""Print the name of facet-app's main landmark (the current page) and whether a dialog is open."""
import warnings
warnings.filterwarnings("ignore")
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
found = {"main": None, "dialog": None}
def walk(n):
    r = n.get_role_name()
    if r == "landmark" and found["main"] is None and n.get_name() not in ("Hardware categories", "Toolbar"): found["main"] = n.get_name()
    if r == "dialog": found["dialog"] = n.get_name()
    for i in range(n.get_child_count()): walk(n.get_child_at_index(i))
d = Atspi.get_desktop(0)
for i in range(d.get_child_count()):
    a = d.get_child_at_index(i)
    if "facet" in (a.get_name() or "").lower():
        walk(a); break
print(f"page={found['main']!r} dialog={found['dialog']!r}")
