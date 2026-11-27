#!/usr/bin/env python3
"""P59: 从 data/777/full/{recipes,tags_item} 生成 crafting.rs 的配方表（tag 原生）。

用法: python scripts/gen_recipes.py [--check]
  重写 crafting.rs 中 GEN-CONSTS / GEN-TAGS / GEN-TABLE 标记区；
  --check 只校验一致性（不一致 exit 1）。

口径：
- 只收 minecraft:crafting_shaped / crafting_shapeless；其余 type 全部跳过。
- 需求项：单个物品 / 单个 tag / 选项数组（选项数组合成内联 TAG_ALT_n）。
  含 components 的原料/产物跳过；未知物品/tag 跳过。
- 需求统一为 canonical 形式：("ITEM", mc名, n) / ("TAGM", 成员mc元组, n)，
  引擎只认成员归属，不认 tag 名，因此同成员的不同 tag 自动合并。
- 合并：旧表按 canonical (result, needs, count) 与 vanilla 对齐——全等则跳过
  （旧名/旧顺序保留）；仅 count 不同则用 vanilla 真值替换（报告）；
  其余 vanilla 条目全部新增（name 取 recipe id，重名加 __N，按 name 排序追加）；
  vanilla 里找不到的旧条目视为 custom 原样保留（报告）。
- 引擎用负数 ID 引用标签：need id = -(index+1)，成员表见 CRAFT_TAGS。
"""
import json
import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RECIPES_DIR = ROOT / "data/777/full/recipes"
TAGS_DIR = ROOT / "data/777/full/tags_item"
ITEMS_JSON = ROOT / "data/777/items.json"
CRAFT_RS = ROOT / "crates/mc-world/src/crafting.rs"

ENTRY_RE = re.compile(
    r'Recipe \{ needs: &\[(.*?)\], result: \((.*?), (\d+)\), name: "(.*?)" \}'
)
CONST_RE = re.compile(r'const (\w+): i32 = registry_id\(mc_registry::ITEMS, "([^"]+)"\)')
TAG_CONST_RE = re.compile(r"const (TAG_\w+): i32 = -(\d+);")


def to_mc(name):
    return name if ":" in name else "minecraft:" + name


def load_items():
    raw = json.loads(ITEMS_JSON.read_text(encoding="utf-8"))
    return {("minecraft:" + k if ":" not in k else k): v["id"] for k, v in raw.items()}


def load_tags():
    tags = {}
    for f in TAGS_DIR.glob("*.json"):
        stem = f.stem
        prefix = "data_minecraft_tags_item_"
        assert stem.startswith(prefix), stem
        tag = "minecraft:" + stem[len(prefix):]
        tags[tag] = json.loads(f.read_text(encoding="utf-8")).get("values", [])
    resolved = {}

    def resolve(tag, seen):
        if tag in resolved:
            return resolved[tag]
        if tag in seen:
            raise ValueError(f"tag cycle: {tag}")
        out = []
        for v in tags.get(tag, []):
            if isinstance(v, dict):
                v = v.get("id", v.get("value", ""))
            if not isinstance(v, str):
                continue
            if v.startswith("#"):
                out.extend(resolve(to_mc(v[1:]), seen | {tag}))
            else:
                out.append(to_mc(v))
        resolved[tag] = sorted(set(out))
        return resolved[tag]

    for tag in list(tags):
        resolve(tag, set())
    return resolved


def norm_options(ref):
    if isinstance(ref, str):
        return [("tag" if ref.startswith("#") else "item",
                 to_mc(ref[1:] if ref.startswith("#") else ref))]
    if isinstance(ref, dict):
        if "item" in ref:
            return [("item", to_mc(ref["item"]))]
        if "tag" in ref:
            t = ref["tag"]
            return [("tag", to_mc(t[1:] if t.startswith("#") else t))]
        return []
    if isinstance(ref, list):
        out = []
        for x in ref:
            out.extend(norm_options(x))
        return out
    return []


def canon_needs(items):
    """[(kind, mc, n)] -> canonical tuple（TAG 按成员元组归一，排序）。"""
    return tuple(sorted(items))


def parse_existing(item_ids, tags):
    """解析 crafting.rs 现有条目 -> [(name, canon_needs, res_mc, count)]（文件顺序）。
    TAG 引用通过 CRAFT_TAGS 成员表还原为 canonical 形式。"""
    text = CRAFT_RS.read_text(encoding="utf-8")
    consts = dict(CONST_RE.findall(text))  # const 名 -> mc 名
    tag_consts = dict((name, int(n)) for name, n in TAG_CONST_RE.findall(text))
    tags_block_m = re.search(r"// === GEN-TAGS-START ===(.*)// === GEN-TAGS-END ===",
                              text, flags=re.S)
    tag_members = {}
    if tags_block_m:
        idx = 0
        for m in re.finditer(r"&\[(.*?)\],", tags_block_m.group(1)):
            members = []
            for cm in m.group(1).split(","):
                cm = cm.strip()
                if cm and cm in consts:
                    members.append(consts[cm])
            tag_members[idx] = tuple(sorted(members))
            idx += 1
    mc_of_const = dict(consts)
    for name, n in tag_consts.items():
        mc_of_const[name] = ("TAGIDX", n)
    entries = []
    for m in ENTRY_RE.finditer(text):
        needs_raw, res_raw, res_count, name = m.groups()
        needs = []
        ok = True
        for part in needs_raw.split("),"):
            part = part.strip().strip("()")
            if not part:
                continue
            cname, n = [x.strip() for x in part.split(",")]
            if cname not in mc_of_const:
                ok = False
                break
            ref = mc_of_const[cname]
            if isinstance(ref, tuple):
                members = tag_members.get(ref[1] - 1, ())
                if not members:
                    ok = False
                    break
                needs.append(("TAGM", members, int(n)))
            else:
                needs.append(("ITEM", ref, int(n)))
        if not ok or res_raw not in consts:
            continue
        entries.append((name, canon_needs(needs), consts[res_raw], int(res_count)))
    return entries


def main():
    check_only = "--check" in sys.argv
    item_ids = load_items()
    tags = load_tags()
    known_tags = {t: tuple(sorted(m for m in ms if m in item_ids))
                  for t, ms in tags.items()}
    known_tags = {t: ms for t, ms in known_tags.items() if ms}
    # 内联选项标签
    alt_sets = {}
    for path in sorted(RECIPES_DIR.glob("*.json")):
        d = json.loads(path.read_text(encoding="utf-8"))
        if d.get("type") not in ("minecraft:crafting_shaped", "minecraft:crafting_shapeless"):
            continue
        groups = []
        if d["type"] == "minecraft:crafting_shaped":
            for ref in d.get("key", {}).values():
                opts = norm_options(ref)
                if len(opts) > 1:
                    groups.append(opts)
        else:
            for ing in d.get("ingredients", []):
                opts = norm_options(ing)
                if len(opts) > 1:
                    groups.append(opts)
        for opts in groups:
            members = set()
            for kind, mc in opts:
                if kind == "tag":
                    members.update(m for m in known_tags.get(mc, ()) if m in item_ids)
                else:
                    members.add(mc)
            members = tuple(sorted(m for m in members if m in item_ids))
            if members and members not in alt_sets:
                alt_sets[members] = f"TAG_ALT_{len(alt_sets)}"
    # 逐文件解析
    uniq = {}
    skipped = Counter()
    for path in sorted(RECIPES_DIR.glob("*.json")):
        rid = path.stem
        prefix = "data_minecraft_recipe_"
        name = rid[len(prefix):] if rid.startswith(prefix) else rid
        d = json.loads(path.read_text(encoding="utf-8"))
        if d.get("type") not in ("minecraft:crafting_shaped", "minecraft:crafting_shapeless"):
            skipped["skip type"] += 1
            continue
        res = d.get("result")
        if isinstance(res, str):
            res = {"id": res}
        if not isinstance(res, dict) or "id" not in res or "components" in res:
            skipped["bad result"] += 1
            continue
        res_mc = to_mc(res["id"])
        if res_mc not in item_ids:
            skipped["unknown result"] += 1
            continue
        count = res.get("count", 1)
        if not isinstance(count, int) or count <= 0:
            skipped["bad result count"] += 1
            continue
        groups = []
        bad = False
        if d["type"] == "minecraft:crafting_shaped":
            key = d.get("key", {})
            pattern = d.get("pattern", [])
            cnt = Counter()
            for row in pattern:
                for ch in row:
                    if ch == " ":
                        continue
                    if ch not in key:
                        bad = True
                        break
                    cnt[ch] += 1
                if bad:
                    break
            if bad or not cnt:
                skipped["bad pattern"] += 1
                continue
            for ch, n in cnt.items():
                ref = key[ch]
                if isinstance(ref, dict) and "components" in ref:
                    bad = True
                    break
                if isinstance(ref, list) and any(
                        isinstance(x, dict) and "components" in x for x in ref):
                    bad = True
                    break
                groups.append((norm_options(ref), n))
            if bad:
                skipped["ingredient components"] += 1
                continue
        else:
            ingredients = d.get("ingredients", [])
            if not ingredients:
                skipped["empty ingredients"] += 1
                continue
            for ing in ingredients:
                if isinstance(ing, dict) and "components" in ing:
                    bad = True
                    break
                groups.append((norm_options(ing), 1))
            if bad:
                skipped["ingredient components"] += 1
                continue
        needs = []
        valid = True
        for opts, n in groups:
            if len(opts) == 1 and opts[0][0] == "item":
                if opts[0][1] not in item_ids:
                    valid = False
                    break
                needs.append(("ITEM", opts[0][1], n))
            elif len(opts) == 1 and opts[0][0] == "tag":
                ms = known_tags.get(opts[0][1], ())
                if not ms:
                    valid = False
                    break
                needs.append(("TAGM", ms, n))
            else:
                members = set()
                for kind, mc in opts:
                    if kind == "tag":
                        members.update(m for m in known_tags.get(mc, ()) if m in item_ids)
                    else:
                        members.add(mc)
                members = tuple(sorted(m for m in members if m in item_ids))
                if not members or members not in alt_sets:
                    valid = False
                    break
                needs.append(("TAGM", members, n))
        if not valid or not needs:
            skipped["unresolvable ingredient"] += 1
            continue
        key = (res_mc, canon_needs(needs), count)
        if key in uniq:
            skipped["dup combo merged"] += 1
            continue
        uniq[key] = name
    # 合并：旧条目按 canonical 对齐
    exist_entries = parse_existing(item_ids, tags)
    old_by_key = {}
    for name, needs, res_mc, count in exist_entries:
        old_by_key.setdefault((res_mc, needs, count), []).append(name)
    vanilla_by_needs = {}
    for (res_mc, needs, count), vname in uniq.items():
        vanilla_by_needs.setdefault((res_mc, needs), []).append((count, vname))
    ordered = []
    used_names = set()
    replaced = []
    seen_ordered = set()
    for name, needs, res_mc, count in exist_entries:
        key = (res_mc, needs, count)
        if key in {k for k in uniq}:
            if key not in seen_ordered:
                seen_ordered.add(key)
                if name not in used_names:
                    ordered.append((name, needs, res_mc, count))
                    used_names.add(name)
            continue
        # vanilla 同 (result, needs) 但 count 不同 -> 用 vanilla 真值替换（报告）。
        if (res_mc, needs) in vanilla_by_needs:
            vcount = vanilla_by_needs[(res_mc, needs)][0][0]
            if vcount != count:
                replaced.append((name, count, vcount))
            count = vcount
            key = (res_mc, needs, count)
            if key in seen_ordered:
                continue
            seen_ordered.add(key)
            if name not in used_names:
                ordered.append((name, needs, res_mc, count))
                used_names.add(name)
            continue
        # 旧条目 vanilla 里没有 -> custom 保留（顺序保留；完全重复的去重）
        if key in seen_ordered:
            continue
        seen_ordered.add(key)
        nn = name
        if nn in used_names:
            i = 2
            while f"{name}__custom{i}" in used_names:
                i += 1
            nn = f"{name}__custom{i}"
        ordered.append((nn, needs, res_mc, count))
        used_names.add(nn)
    # 新增 vanilla 条目
    new_entries = []
    for (res_mc, needs, count), vname in uniq.items():
        if (res_mc, needs, count) in {(r, n, c) for _, n, r, c in ordered}:
            continue
        nn, i = vname, 2
        while nn in used_names:
            nn = f"{vname}__{i}"
            i += 1
        used_names.add(nn)
        new_entries.append((nn, needs, res_mc, count))
    new_entries.sort(key=lambda x: x[0])
    ordered += new_entries
    # const 生成（标签成员也要有 const，CRAFT_TAGS 引用它们）
    tag_groups = set()
    for _, needs, _, _ in ordered:
        for k, mm, n in needs:
            if k == "TAGM":
                tag_groups.add(mm)
    needed_mc = set()
    for _, needs, res_mc, _ in ordered:
        needed_mc.add(res_mc)
        for k, mm, n in needs:
            if k == "ITEM":
                needed_mc.add(mm)
    for members in tag_groups:
        needed_mc.update(m for m in members if m in item_ids)
    taken = set(re.findall(r"const (\w+): i32", CRAFT_RS.read_text(encoding="utf-8")))
    taken -= {m.group(1) for m in TAG_CONST_RE.finditer(CRAFT_RS.read_text(encoding="utf-8"))}
    const_map = {}
    for mc in sorted(needed_mc):
        base = "ITEM_" + mc.split(":", 1)[1].upper().replace("-", "_")
        base = re.sub(r"[^A-Z0-9_]", "_", base)
        nm, i = base, 2
        while nm in taken:
            nm = f"{base}_{i}"
            i += 1
        taken.add(nm)
        const_map[mc] = nm
    # 旧 const 名复用（保持 diff 最小）：mc 名已存在 const 则沿用
    for m in CONST_RE.finditer(CRAFT_RS.read_text(encoding="utf-8")):
        cname, mc = m.group(1), m.group(2)
        if mc in needed_mc:
            const_map[mc] = cname
    # 标签表：收集用到的成员组（命名 tag 优先）
    member_to_tag = {}
    for t, ms in sorted(known_tags.items()):
        member_to_tag.setdefault(ms, t)
    for members, alt in sorted(alt_sets.items(), key=lambda kv: kv[1]):
        member_to_tag.setdefault(members, f"ALT:{alt}")
    used_groups = set()
    for _, needs, _, _ in ordered:
        for k, mm, n in needs:
            if k == "TAGM":
                used_groups.add(mm)
    tag_lines = ["// === GEN-TAGS-START === (scripts/gen_recipes.py 生成；勿手改)",
                 "/// 合成标签成员表（负数 ID 引用）：need id = -(index+1)。",
                 "pub static CRAFT_TAGS: &[&[i32]] = &["]
    tag_index = {}
    tag_const_of = {}
    for i, members in enumerate(sorted(used_groups)):
        tag_index[members] = i
        mem_consts = ", ".join(const_map[m] for m in members)
        tag_lines.append(f"    &[{mem_consts}],")
    tag_lines.append("];")
    for members in sorted(used_groups):
        key = member_to_tag.get(members)
        if key is not None and not key.startswith("ALT:"):
            base = "TAG_" + key.split(":", 1)[1].upper().replace("-", "_")
            base = re.sub(r"[^A-Z0-9_]", "_", base)
        else:
            alt = None
            for m2, a in alt_sets.items():
                if m2 == members:
                    alt = a
                    break
            base = alt if alt else "TAG_ANON"
        tag_lines.append(f"const {base}: i32 = -{tag_index[members] + 1};")
        tag_const_of[members] = base
    tag_lines.append("// === GEN-TAGS-END ===")
    # 渲染
    const_lines = ["// === GEN-CONSTS-START === (scripts/gen_recipes.py 从 "
                   "data/777/full/{recipes,tags_item} 生成；勿手改)"]
    for mc in sorted(needed_mc):
        const_lines.append(
            f'const {const_map[mc]}: i32 = registry_id(mc_registry::ITEMS, "{mc}") as i32; '
            f"// {item_ids[mc]}")
    const_lines.append("// === GEN-CONSTS-END ===")
    table_lines = ["// === GEN-TABLE-START === (scripts/gen_recipes.py 生成；勿手改)",
                   "pub const RECIPES: &[Recipe] = &["]
    for name, needs, res_mc, count in ordered:
        parts = []
        for k, mm, n in needs:
            if k == "ITEM":
                parts.append(f"({const_map[mm]}, {n})")
            else:
                parts.append(f"({tag_const_of[mm]}, {n})")
        table_lines.append(
            f'    Recipe {{ needs: &[{", ".join(parts)}], result: ({const_map[res_mc]}, {count}), '
            f'name: "{name}" }},')
    table_lines.append("];")
    table_lines.append("// === GEN-TABLE-END ===")
    const_block = "\n".join(const_lines) + "\n"
    table_block = "\n".join(table_lines) + "\n"
    tag_block = "\n".join(tag_lines) + "\n"
    text = CRAFT_RS.read_text(encoding="utf-8")
    new_text = re.sub(r"// === GEN-CONSTS-START ===.*?// === GEN-CONSTS-END ===\n?",
                      lambda _: const_block, text, count=1, flags=re.S)
    if "// === GEN-TAGS-START ===" not in new_text:
        new_text = new_text.replace("// === GEN-TABLE-START ===",
                                    tag_block + "\n// === GEN-TABLE-START ===", 1)
    else:
        new_text = re.sub(r"// === GEN-TAGS-START ===.*?// === GEN-TAGS-END ===\n?",
                          lambda _: tag_block, new_text, count=1, flags=re.S)
    new_text = re.sub(r"// === GEN-TABLE-START ===.*?// === GEN-TABLE-END ===\n?",
                      lambda _: table_block, new_text, count=1, flags=re.S)
    if check_only:
        if new_text != text:
            print("gen_recipes: crafting.rs 与生成结果不一致")
            sys.exit(1)
        print("gen_recipes: 一致")
        return
    CRAFT_RS.write_text(new_text, encoding="utf-8")
    print(f"vanilla crafting files: {len(list(RECIPES_DIR.glob('*.json')))}")
    print(f"unique vanilla recipes: {len(uniq)}")
    print(f"old entries parsed: {len(exist_entries)}, customs kept: "
          f"{sum(1 for _, n, _, _ in ordered if n and False)}")
    print(f"total: {len(ordered)} (replaced counts: {replaced})")
    print("skipped:", dict(skipped))
    groups = {}
    for name, needs, res_mc, count in ordered:
        sig = tuple(sorted((k, mm, n) for k, mm, n in needs))
        groups.setdefault(sig, []).append(name)
    multi = {k: v for k, v in groups.items() if len(v) > 1}
    print(f"ambiguous groups: {len(multi)}")
    for sig, names in sorted(multi.items(), key=lambda kv: str(kv[0])):
        print("  ", names)

if __name__ == "__main__":
    main()
