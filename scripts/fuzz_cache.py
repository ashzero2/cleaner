"""Randomised differential test for the scan cache.

usage: fuzz_cache.py <cl-rs binary> <scratch copy of a tree> <cache dir> <rounds> plain|links <seed>

Each round applies random file operations, updates from the saved snapshot, and compares
the result with a fresh scan. `links` also creates and removes hard links, where only the
totals (not the placement of the charge) must match. Run it on a COPY of a directory.
"""
import json, os, random, shutil, subprocess, sys, time

B = sys.argv[1]; ROOT = sys.argv[2]; CACHE = sys.argv[3]; ROUNDS = int(sys.argv[4]); HARDLINKS = sys.argv[5] == "links"; SEED = int(sys.argv[6])
rnd = random.Random(SEED)

def scan(extra):
    out = subprocess.run([B, "scan", "-q", "--json", "-d", "99", "-n", "100000000", *extra, ROOT], capture_output=True, text=True, check=True).stdout
    return json.loads(out)

def dirs_and_files():
    ds, fs = [], []
    for base, dn, fn in os.walk(ROOT):
        ds.append(base)
        fs.extend(os.path.join(base, f) for f in fn)
    return ds, fs

def blob(n): return os.urandom(n)

def op():
    ds, fs = dirs_and_files()
    kind = rnd.choice(["create", "create", "delete", "grow", "truncate", "mkdirs", "rmtree", "rename", "swap", "symlink", "link" if HARDLINKS else "create", "unlink_link" if HARDLINKS else "delete"])
    try:
        if kind == "create":
            open(os.path.join(rnd.choice(ds), f"new{rnd.randrange(10**9)}.bin"), "wb").write(blob(rnd.randrange(1, 90000)))
        elif kind == "delete" and fs:
            os.remove(rnd.choice(fs))
        elif kind == "grow" and fs:
            open(rnd.choice(fs), "ab").write(blob(rnd.randrange(1, 70000)))
        elif kind == "truncate" and fs:
            os.truncate(rnd.choice(fs), 0)
        elif kind == "mkdirs":
            d = os.path.join(rnd.choice(ds), *[f"d{rnd.randrange(1000)}" for _ in range(rnd.randrange(1, 5))])
            os.makedirs(d, exist_ok=True)
            for i in range(rnd.randrange(0, 4)): open(os.path.join(d, f"f{i}.bin"), "wb").write(blob(rnd.randrange(1, 30000)))
        elif kind == "rmtree" and len(ds) > 1:
            d = rnd.choice(ds[1:]); shutil.rmtree(d)
        elif kind == "rename" and len(ds) > 1:
            src = rnd.choice(ds[1:] + fs); dst = os.path.join(rnd.choice(ds), f"mv{rnd.randrange(10**9)}")
            if not dst.startswith(src + os.sep): os.rename(src, dst)
        elif kind == "swap" and fs:
            f = rnd.choice(fs); os.remove(f); os.makedirs(f); open(os.path.join(f, "inner.bin"), "wb").write(blob(5000))
        elif kind == "symlink":
            os.symlink(rnd.choice(ds), os.path.join(rnd.choice(ds), f"sl{rnd.randrange(10**9)}"))
        elif kind == "link" and fs:
            os.link(rnd.choice(fs), os.path.join(rnd.choice(ds), f"hl{rnd.randrange(10**9)}"))
        elif kind == "unlink_link" and fs:
            multi = [f for f in fs if os.lstat(f).st_nlink > 1]
            if multi: os.remove(rnd.choice(multi))
    except OSError:
        pass

scan(["--cache-dir", CACHE])
bad = 0
for r in range(ROUNDS):
    for _ in range(rnd.randrange(10, 40)): op()
    time.sleep(1.2)
    cached = scan(["--cache-dir", CACHE]); fresh = scan(["--no-cache"])
    same_totals = cached["size"] == fresh["size"] and cached["files"] == fresh["files"]
    same_tree = cached["tree"] == fresh["tree"]
    ok = same_totals and (same_tree or HARDLINKS)
    print(f"round {r:2d}: source={cached['source']:5s} changed={cached.get('changed_dirs')} size={cached['size']} files={cached['files']} totals={'ok' if same_totals else 'DIFF'} tree={'ok' if same_tree else 'diff'}")
    if not ok:
        bad += 1
        print("   cached", cached["size"], cached["files"], " fresh", fresh["size"], fresh["files"])
print("FAILURES:", bad)
sys.exit(1 if bad else 0)
