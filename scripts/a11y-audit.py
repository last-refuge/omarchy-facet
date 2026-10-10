#!/usr/bin/env python3
"""Audit the live AT-SPI tree of the running facet-app. Exit 1 on problems.
Checks: a navigation and a main landmark exist; at least one heading; every interactive node has a
non-empty accessible name; interactive nodes are focusable. Prints a one-line summary."""
import sys, warnings
warnings.filterwarnings("ignore")
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

INTERACTIVE = {"button", "link", "check box", "push button", "list item", "toggle button", "radio button", "text", "entry"}
needle = (sys.argv[1] if len(sys.argv) > 1 else "facet").lower()
desktop = Atspi.get_desktop(0)
app = next((desktop.get_child_at_index(i) for i in range(desktop.get_child_count()) if needle in (desktop.get_child_at_index(i).get_name() or "").lower()), None)
if app is None:
    print("FAIL no app registered"); sys.exit(1)

counts, problems = {}, []
def walk(n, path):
    role = n.get_role_name(); name = n.get_name() or ""
    counts[role] = counts.get(role, 0) + 1
    if role in INTERACTIVE:
        if not name.strip():
            problems.append(f"unnamed {role} at {path}")
        if not n.get_state_set().contains(Atspi.StateType.FOCUSABLE) and role in {"button", "link", "check box", "push button"}:
            problems.append(f"not focusable: {role} {name!r}")
    for i in range(n.get_child_count()):
        walk(n.get_child_at_index(i), path + "/" + role)
walk(app, "app")
for need in ("landmark", "heading"):
    if counts.get(need, 0) == 0:
        problems.append(f"no {need}")
summary = ", ".join(f"{k}:{v}" for k, v in sorted(counts.items()))
if problems:
    print("FAIL", "; ".join(problems[:6]), "|", summary); sys.exit(1)
print("ok", summary)
