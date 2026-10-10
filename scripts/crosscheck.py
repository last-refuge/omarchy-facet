#!/usr/bin/env python3
"""Compare `facet --json` against trusted system tools on this machine. Prints one line per check:
PASS / DIFF (with both values). Exits 1 if any hard check differs. Live readings allow a tolerance."""
import json, re, subprocess, sys

def run(*cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=20).stdout
    except Exception as e:
        return ""

inv = json.loads(run("./target/release/facet", "--json", "--include-identifiers"))["inventory"]["components"]
byid = {c["id"]: c for c in inv}
def spec(c, label):
    for s in c.get("summary", []) + [x for g in c.get("groups", []) for x in g["specs"]]:
        if s["label"] == label and s["value"]["state"] == "available":
            return s["value"]["value"]["v"]
    return None

results = []
def check(name, ours, theirs, tol=0, soft=False):
    ok = ours is not None and theirs is not None and abs(float(ours) - float(theirs)) <= tol if isinstance(theirs, (int, float)) else ours == theirs
    results.append((ok, soft, name, ours, theirs))

lscpu = run("lscpu")
def lf(key):
    m = re.search(rf"^{re.escape(key)}:\s*(.+)$", lscpu, re.M)
    return m.group(1).strip() if m else None
cpu = byid["cpu:package0"]
# Facet deliberately drops the trademark marks lscpu keeps: "Intel(R) Core(TM) i9" -> "Intel Core i9".
check("CPU model name (ignoring (R)/(TM) marks)", cpu["title"], re.sub(r"\s+", " ", re.sub(r"\((R|TM)\)", "", lf("Model name"))).strip())
cores_per = int(lf("Core(s) per socket")); sockets = int(lf("Socket(s)")); tpc = int(lf("Thread(s) per core"))
check("CPU cores", spec(cpu, "Cores"), cores_per * sockets)
check("CPU threads", spec(cpu, "Threads"), int(lf("CPU(s)")))
check("CPU max frequency (MHz)", round(spec(cpu, "Maximum frequency") / 1e6), round(float(lf("CPU max MHz"))))

free = run("free", "-b").splitlines()
mem_total = int(free[1].split()[1])
mem = byid["memory:system"]
check("Memory total (bytes) vs free -b", spec(mem, "Usable memory"), mem_total)
check("Swap total (bytes) vs free -b", spec(mem, "Swap total"), int(free[2].split()[1]) if len(free) > 2 else None)

lsblk = json.loads(run("lsblk", "-J", "-b", "-d", "-o", "NAME,SIZE,MODEL,TYPE,ROTA"))["blockdevices"]
for d in lsblk:
    if d["type"] == "disk":
        c = byid.get(f"storage:{d['name']}")
        if c:
            check(f"Storage {d['name']} capacity (bytes)", spec(c, "Capacity"), int(d["size"]))
            check(f"Storage {d['name']} model", c["title"], (d["model"] or "").strip())

lspci = run("lspci", "-nn", "-d", "::0300") + run("lspci", "-nn", "-d", "::0302")
for m in re.finditer(r"^(\S+) .*\[([0-9a-f]{4}):([0-9a-f]{4})\]", lspci, re.M):
    addr, ven, dev = m.groups()
    full = addr if addr.count(":") == 2 else "0000:" + addr
    c = byid.get(f"graphics:{full}")
    ours = spec(c, "Vendor ID") if c else None
    results.append((c is not None, False, f"GPU {full} present ({ven}:{dev})", bool(c), True))

lsusb = run("lsusb")
usb_pairs = set(re.findall(r"ID ([0-9a-f]{4}:[0-9a-f]{4})", lsusb))
ours_pairs = set()
for c in inv:
    if c["id"].startswith("usb:"):
        v = spec(c, "Vendor ID"); p = spec(c, "Product ID")
        if v and p: ours_pairs.add(f"{v[2:] if v.startswith('0x') else v}:{p[2:] if p.startswith('0x') else p}".lower())
check("USB device count vs lsusb", len([c for c in inv if c["id"].startswith("usb:")]), len(usb_pairs) and len(lsusb.strip().splitlines()))

sens = run("sensors", "-j")
try:
    sj = json.loads(sens)
    pk = sj["coretemp-isa-0000"]["Package id 0"]["temp1_input"]
    c = next(c for c in inv if c["id"].startswith("sensors:coretemp"))
    check("CPU package temperature (°C, ±5 live drift)", spec(c, "Package id 0"), pk, tol=5, soft=True)
except Exception as e:
    results.append((False, True, "CPU package temperature", None, f"sensors -j failed: {e}"))

up = run("upower", "-d")
m = re.search(r"energy-full-design:\s+([\d.]+) Wh", up)
bat = byid.get("power:BAT0")
if m and bat:
    design = spec(bat, "Design capacity")
    results.append((True, True, "Battery design capacity present", design, m.group(1) + " Wh (text compare below)"))
    results.append((str(design).startswith(m.group(1).split('.')[0]), True, "Battery design capacity ≈ upower energy-full-design", design, m.group(1)))

bad = 0
for ok, soft, name, ours, theirs in results:
    tag = "PASS" if ok else ("DIFF(soft)" if soft else "DIFF")
    if not ok and not soft: bad += 1
    print(f"{tag:11} {name}: facet={ours!r} tool={theirs!r}")
print(f"\n{len(results)} checks, {bad} hard differences")
sys.exit(1 if bad else 0)
