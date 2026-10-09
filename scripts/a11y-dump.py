#!/usr/bin/env python3
"""Dump the live AT-SPI accessibility tree of a running app. Usage: a11y-dump.py <app-name-substring>"""
import sys
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

needle = (sys.argv[1] if len(sys.argv) > 1 else "facet").lower()

def dump(node, depth=0):
    try:
        role = node.get_role_name()
        name = node.get_name()
        states = node.get_state_set()
        flags = [n for n, s in (("focusable", Atspi.StateType.FOCUSABLE), ("focused", Atspi.StateType.FOCUSED), ("showing", Atspi.StateType.SHOWING)) if states.contains(s)]
        desc = node.get_description()
        actions = node.get_action_iface()
        acts = [actions.get_action_name(i) for i in range(actions.get_n_actions())] if actions else []
        print(f"{'  ' * depth}[{role}] {name!r}" + (f" desc={desc!r}" if desc else "") + (f" {flags}" if flags else "") + (f" actions={acts}" if acts else ""))
        for i in range(node.get_child_count()):
            dump(node.get_child_at_index(i), depth + 1)
    except Exception as e:
        print(f"{'  ' * depth}<error {e}>")

desktop = Atspi.get_desktop(0)
found = False
for i in range(desktop.get_child_count()):
    app = desktop.get_child_at_index(i)
    if app and needle in (app.get_name() or "").lower():
        found = True
        dump(app)
if not found:
    names = [desktop.get_child_at_index(i).get_name() for i in range(desktop.get_child_count())]
    print(f"no app matching {needle!r}; registered apps: {names}")
    sys.exit(1)
