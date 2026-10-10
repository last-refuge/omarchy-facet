#!/usr/bin/env python3
"""Print 'role: name' of the AT-SPI element that currently has keyboard focus in facet-app."""
import sys, warnings
warnings.filterwarnings("ignore")
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

def find(n):
    try:
        if n.get_state_set().contains(Atspi.StateType.FOCUSED) and n.get_role_name() not in ("frame", "application"):
            return n
        for i in range(n.get_child_count()):
            r = find(n.get_child_at_index(i))
            if r: return r
    except Exception:
        pass
    return None

d = Atspi.get_desktop(0)
for i in range(d.get_child_count()):
    a = d.get_child_at_index(i)
    if "facet" in (a.get_name() or "").lower():
        f = find(a)
        print(f"{f.get_role_name()}: {f.get_name()}" if f else "(nothing focused)")
        break
else:
    print("(no facet app)")
