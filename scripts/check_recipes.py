#!/usr/bin/env python3
"""校验 crafting.rs 生成表：vanilla 全覆盖、无完全重复、custom 有名有姓。"""
import json
import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import importlib.util
spec = importlib.util.spec_from_file_location("gen", str(ROOT / "scripts/gen_recipes.py"))
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)

item_ids = gen.load_items()
tags = gen.load_tags()
known_tags = {t: tuple(sorted(m for m in ms if m in item_ids))
              for t, ms in tags.items()}
known_tags = {t: ms for t, ms in known_tags.items() if ms}

# vanilla 全集（canonical）
van = {}
files = sorted((ROOT / "data/777/full/recipes").glob("*.json"))
for path in files:
    d = json.loads(path.read_text(encoding="utf-8"))
    if d.get("type") not in ("minecraft:crafting_shaped", "minecraft:crafting_shapeless"):
        continue
    res = d.get("result")
    if isinstance(res, str):
        res = {"id": res}
    if not isinstance(res, dict) or "id" not in res or "components" in res:
        continue
    res_mc = gen.to_mc(res["id"])
    if res_mc not in item_ids:
        continue
    count = res.get("count", 1)
    groups = []
    if d["type"] == "minecraft:crafting_shaped":
        key, pattern = d.get("key", {}), d.get("pattern", [])
        cnt = Counter()
        for row in pattern:
            for ch in row:
                if ch != " ":
                    cnt[ch] += 1
        groups = [(gen.norm_options(key[ch]), n) for ch, n in cnt.items()]
    else:
        for ing in d.get("ingredients", []):
            groups.append((gen.norm_options(ing), 1))
    needs = []
    ok = True
    for opts, n in groups:
        if len(opts) == 1 and opts[0][0] == "item" and opts[0][1] in item_ids:
            needs.append(("ITEM", opts[0][1], n))
        elif len(opts) == 1 and opts[0][0] == "tag" and known_tags.get(opts[0][1]):
            needs.append(("TAGM", known_tags[opts[0][1]], n))
        else:
            members = set()
            for kind, mc in opts:
                if kind == "tag":
                    members.update(m for m in known_tags.get(mc, ()) if m in item_ids)
                else:
                    members.add(mc)
            members = tuple(sorted(m for m in members if m in item_ids))
            if not members:
                ok = False
                break
            needs.append(("TAGM", members, n))
    if not ok or not needs:
        continue
    van[(res_mc, tuple(sorted(needs)), count)] = path.stem

# 表内条目
text = (ROOT / "crates/mc-world/src/crafting.rs").read_text(encoding="utf-8")
consts = dict(gen.CONST_RE.findall(text))  # const 名 -> mc 名
tag_consts = dict((name, int(n)) for name, n in gen.TAG_CONST_RE.findall(text))
m = re.search(r"// === GEN-TAGS-START ===(.*)// === GEN-TAGS-END ===", text, flags=re.S)
tagmem = {}
idx = 0
for mm in re.finditer(r"&\[(.*?)\],", m.group(1)):
    mems = tuple(sorted(consts[c.strip()] for c in mm.group(1).split(",") if c.strip()))
    tagmem[idx] = mems
    idx += 1
have = Counter()
missing = []
for em in gen.ENTRY_RE.finditer(text):
    needs_raw, res_raw, res_count, name = em.groups()
    needs = []
    for part in needs_raw.split("),"):
        part = part.strip().strip("()")
        if not part:
            continue
        cname, n = [x.strip() for x in part.split(",")]
        if cname not in consts and cname not in tag_consts:
            needs.append(("??", cname, int(n)))
            continue
        if cname in tag_consts:
            needs.append(("TAGM", tagmem[tag_consts[cname] - 1], int(n)))
        else:
            needs.append(("ITEM", consts[cname], int(n)))
    key = (consts.get(res_raw, res_raw), tuple(sorted(needs)), int(res_count))
    have[key] += 1
dups = {k: v for k, v in have.items() if v > 1}
not_covered = [k for k in van if k not in have]
print(f"vanilla: {len(van)}, table distinct keys: {len(have)}")
print(f"exact duplicates in table: {len(dups)}")
for k, v in list(dups.items())[:10]:
    print("  DUP", v, k[0], k[2])
print(f"vanilla recipes missing from table: {len(not_covered)}")
for k in not_covered[:20]:
    print("  MISS", k[0], k[2], van[k])
sys.exit(1 if dups or not_covered else 0)
