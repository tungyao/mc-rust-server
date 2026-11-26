//! mc-registry build script: 从 `data/manifest.json` + `data/777/*.json` 生成真实 ID 注册表。
//!
//! 数据来源是 Pumpkin 26.3 Extractor 提取的 JSON（见 `data/777/SOURCE.md`）。
//! 生成 `$OUT_DIR/generated.rs`，含 BLOCKS / ITEMS / ENTITIES / PACKETS 表以及
//! `by_name` / `by_id` 查找函数；`src/lib.rs` 通过 `include!` 引入并在此之上实现公共 API。
//!
//! 构建期同时执行 P0 校验：把 JSON 里的 ID 与 HANDOFF / 计划文档中的真机实测值逐项比对，
//! 数据漂移会直接导致编译失败，而不是运行期静默错位。
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const EXPECTED_VERSION: &str = "26.3";
const EXPECTED_PROTOCOL: u32 = 777;
const EXPECTED_DATA_VERSION: u32 = 5023;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let data_root = manifest_dir.join("../../data");
    let manifest_path = data_root.join("manifest.json");

    println!("cargo:rerun-if-changed={}", data_root.display());
    println!("cargo:rerun-if-changed={}", manifest_path.display());

    let manifest = read_json(&manifest_path);
    assert_eq!(
        manifest.get("version").and_then(Value::as_str),
        Some(EXPECTED_VERSION),
        "data/manifest.json version mismatch"
    );
    assert_eq!(
        as_u32(manifest.get("protocol"), "manifest.protocol"),
        EXPECTED_PROTOCOL,
        "data/manifest.json protocol mismatch"
    );
    assert_eq!(
        as_u32(manifest.get("data_version"), "manifest.data_version"),
        EXPECTED_DATA_VERSION,
        "data/manifest.json data_version mismatch"
    );
    let dir = manifest
        .get("data_dir")
        .and_then(Value::as_str)
        .expect("manifest.data_dir must be a string");
    assert!(
        dir.split('/').all(|p| !p.is_empty() && p != "." && p != ".."),
        "manifest.data_dir must be a simple relative directory name"
    );
    let version_dir = data_root.join(dir);
    assert!(
        version_dir.is_dir(),
        "data directory missing: {}",
        version_dir.display()
    );

    let blocks_path = version_dir.join("blocks.json");
    let full_blocks_path = version_dir.join("full").join("blocks.json");
    let items_path = version_dir.join("items.json");
    let entities_path = version_dir.join("entities.json");
    let packets_path = version_dir.join("packets.json");
    let properties_path = version_dir.join("properties.json");
    for p in [
        &blocks_path,
        &full_blocks_path,
        &items_path,
        &entities_path,
        &packets_path,
        &properties_path,
    ] {
        println!("cargo:rerun-if-changed={}", p.display());
    }

    let blocks = read_blocks(&blocks_path);
    let block_states = read_full_blocks(&full_blocks_path);
    let items = read_id_map(&items_path, "items");
    let entities = read_id_map(&entities_path, "entities");
    let packets = read_packets(&packets_path);

    // properties.json 当前不参与 codegen，但仍校验它存在且形状正确，
    // 保证升级时五项数据必须一起更新。
    let properties = read_json(&properties_path);
    assert!(
        properties.as_array().is_some_and(|a| !a.is_empty()),
        "properties.json must be a non-empty array"
    );

    validate_expected_ids(&blocks, &items, &packets);
    validate_block_states(&block_states, &blocks);

    let dest = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"))
        .join("generated.rs");
    fs::write(&dest, render(&blocks, &block_states, &items, &entities, &packets))
        .expect("write $OUT_DIR/generated.rs");
}

// ---------------- 数据读取 ----------------

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse JSON {}: {e}", path.display()))
}

fn as_u32(v: Option<&Value>, ctx: &str) -> u32 {
    v.and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or_else(|| panic!("{ctx}: expected non-negative integer"))
}

fn as_i32(v: Option<&Value>, ctx: &str) -> i32 {
    v.and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or_else(|| panic!("{ctx}: expected i32"))
}

/// 上游名称不带命名空间；统一补 `minecraft:`（若已带则原样保留）。
fn namespaced(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

fn read_blocks(path: &Path) -> Vec<(u32, String)> {
    let v = read_json(path);
    let arr = v
        .get("blocks")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{}: missing `blocks` array", path.display()));
    let mut out = Vec::with_capacity(arr.len());
    for b in arr {
        let name = b
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{}: block missing `name`", path.display()));
        // 关键：blockstate ID 取 default_state_id，不是 id，也不是 states[0].id。
        let id = as_u32(
            b.get("default_state_id"),
            &format!("{}: {name}.default_state_id", path.display()),
        );
        out.push((id, namespaced(name)));
    }
    sort_and_check(&mut out, "blocks");
    out
}

/// P37：`full/blocks.json` 的每个方块全部 blockstate（按 id 升序连续排列）。
/// 生成 `(first_state_id, namespaced_name, state_count)`，供按属性索引解析阶段 ID
/// （如 cocoa 的 age/facing、bamboo 的 age/leaves/stage），不再手写数字。
fn read_full_blocks(path: &Path) -> Vec<(u32, String, u16)> {
    let v = read_json(path);
    let arr = v
        .get("blocks")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{}: missing `blocks` array", path.display()));
    let mut out = Vec::with_capacity(arr.len());
    for b in arr {
        let name = b
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{}: block missing `name`", path.display()));
        let states = b
            .get("states")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{}: {name} missing `states`", path.display()));
        assert!(!states.is_empty(), "{}: {name} has no states", path.display());
        let first = as_u32(
            states[0].get("id"),
            &format!("{}: {name}.states[0].id", path.display()),
        );
        let count = u16::try_from(states.len()).expect("blockstate count fits u16");
        for (i, state) in states.iter().enumerate() {
            assert_eq!(
                as_u32(state.get("id"), &format!("{}: {name}.states[{i}].id", path.display())),
                first + i as u32,
                "{}: {name} blockstates must be contiguous",
                path.display()
            );
        }
        out.push((first, namespaced(name), count));
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    let mut seen = HashMap::new();
    for (_, name, _) in &out {
        assert!(seen.insert(name.as_str(), true).is_none(), "duplicate full block {name}");
    }
    out
}

/// 防 codegen 漂移：默认 state 必须落在该方块的状态区间内；P37 依赖的阶段
/// 索引（age=N 偏移）也要与已知 JSON 实测值一致，漂移直接编译失败。
fn validate_block_states(states: &[(u32, String, u16)], blocks: &[(u32, String)]) {
    for (first, name, count) in states {
        let default = blocks
            .iter()
            .find(|(_, n)| n == name)
            .unwrap_or_else(|| panic!("full block {name} missing from blocks.json"))
            .0;
        assert!(
            (*first..*first + u32::from(*count)).contains(&default),
            "default state {default} outside {name} range {first}..={}",
            *first + u32::from(*count) - 1
        );
    }
    let at = |name: &str, index: u32| -> u32 {
        let (first, _, count) = states
            .iter()
            .find(|(_, n, _)| n == name)
            .unwrap_or_else(|| panic!("missing full block {name}"));
        assert!(index < u32::from(*count), "{name} state index {index} out of range");
        first + index
    };
    for (name, index, expected) in [
        ("minecraft:wheat", 7, 6874),
        ("minecraft:carrots", 7, 12409),
        ("minecraft:potatoes", 7, 12417),
        ("minecraft:beetroots", 3, 16795),
        ("minecraft:nether_wart", 3, 11184),
        ("minecraft:melon_stem", 7, 10091),
        ("minecraft:pumpkin_stem", 7, 10083),
        ("minecraft:sweet_berry_bush", 3, 24301),
        ("minecraft:torchflower_crop", 1, 16779),
        ("minecraft:pitcher_crop", 9, 16789),
        ("minecraft:cocoa", 11, 11226),
        ("minecraft:sugar_cane", 15, 8632),
        ("minecraft:cactus", 15, 8614),
        ("minecraft:bamboo", 11, 18647),
        ("minecraft:mangrove_propagule", 0, 46),
    ] {
        assert_eq!(at(name, index), expected, "state index drift: {name}[{index}]");
    }
}

fn read_id_map(path: &Path, kind: &str) -> Vec<(u32, String)> {
    let v = read_json(path);
    let obj = v
        .as_object()
        .unwrap_or_else(|| panic!("{}: expected top-level object", path.display()));
    let mut out = Vec::with_capacity(obj.len());
    for (name, entry) in obj {
        let id = as_u32(
            entry.get("id"),
            &format!("{}: {name}.id", path.display()),
        );
        out.push((id, namespaced(name)));
    }
    sort_and_check(&mut out, kind);
    out
}

/// PACKETS 的三元组是 (state, name, id)，其中 state = `"<side>.<state>"`
/// （如 `clientbound.play`），与 `packet_id(side, state, name)` 对应。
fn read_packets(path: &Path) -> Vec<(String, String, i32)> {
    let v = read_json(path);
    assert_eq!(
        as_u32(v.get("version"), "packets.version"),
        EXPECTED_PROTOCOL,
        "packets.json protocol mismatch"
    );
    let mut out = Vec::new();
    for side in ["serverbound", "clientbound"] {
        let states = v
            .get(side)
            .and_then(Value::as_object)
            .unwrap_or_else(|| panic!("{}: missing `{side}` object", path.display()));
        for (state, packets) in states {
            let map = packets
                .as_object()
                .unwrap_or_else(|| panic!("{side}.{state} must be an object"));
            for (name, id) in map {
                out.push((
                    format!("{side}.{state}"),
                    name.clone(),
                    as_i32(Some(id), &format!("{side}.{state}.{name}")),
                ));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut seen = HashMap::new();
    for (state, name, _) in &out {
        assert!(
            seen.insert((state.as_str(), name.as_str()), true).is_none(),
            "duplicate packet {state}/{name}"
        );
    }
    out
}

fn sort_and_check(entries: &mut [(u32, String)], kind: &str) {
    entries.sort_by_key(|(id, _)| *id);
    let mut ids = HashMap::new();
    let mut names = HashMap::new();
    for (id, name) in entries.iter() {
        if let Some(prev) = ids.insert(*id, name.as_str()) {
            panic!("{kind}: duplicate ID {id} ({prev} / {name})");
        }
        if let Some(prev) = names.insert(name.as_str(), *id) {
            panic!("{kind}: duplicate name {name} (IDs {prev} / {id})");
        }
    }
}

// ---------------- P0 构建期校验 ----------------

fn validate_expected_ids(
    blocks: &[(u32, String)],
    items: &[(u32, String)],
    packets: &[(String, String, i32)],
) {
    let block_ids: HashMap<&str, u32> = blocks
        .iter()
        .map(|(id, name)| (name.as_str(), *id))
        .collect();
    for (name, expected) in [
        ("minecraft:air", 0),
        ("minecraft:stone", 1),
        ("minecraft:grass_block", 9),
        ("minecraft:dirt", 10),
        ("minecraft:water", 89),
        ("minecraft:sand", 121),
        ("minecraft:oak_log", 140),
        ("minecraft:oak_leaves", 294),
        ("minecraft:coal_ore", 136),
        ("minecraft:iron_ore", 134),
    ] {
        assert_eq!(
            block_ids.get(name).copied(),
            Some(expected),
            "block default_state_id drift: {name}"
        );
    }

    let item_ids: HashMap<&str, u32> = items
        .iter()
        .map(|(id, name)| (name.as_str(), *id))
        .collect();
    for (name, expected) in [
        ("minecraft:stone", 1),
        ("minecraft:dirt", 55),
        ("minecraft:cobblestone", 62),
        ("minecraft:oak_sapling", 77),
        ("minecraft:sand", 88),
        ("minecraft:oak_log", 163),
        ("minecraft:coal", 1010),
        ("minecraft:stick", 1060),
        ("minecraft:raw_iron", 1017),
    ] {
        assert_eq!(
            item_ids.get(name).copied(),
            Some(expected),
            "item ID drift: {name}"
        );
    }

    for (side, state, name, expected) in [
        ("clientbound", "play", "add_entity", 1),
        ("clientbound", "play", "block_changed_ack", 4),
        ("clientbound", "play", "block_update", 8),
        ("clientbound", "play", "level_chunk_with_light", 46),
        ("clientbound", "play", "entity_position_sync", 35),
        ("clientbound", "play", "set_entity_data", 101),
        ("clientbound", "play", "take_item_entity", 127),
        ("serverbound", "play", "player_action", 41),
        ("serverbound", "play", "use_item_on", 66),
        ("serverbound", "play", "move_player_pos", 30),
    ] {
        let key = format!("{side}.{state}");
        let found = packets
            .iter()
            .find(|(s, n, _)| s == &key && n == name)
            .map(|(_, _, id)| *id);
        assert_eq!(found, Some(expected), "packet ID drift: {side}/{state}/{name}");
    }
}

// ---------------- 代码生成 ----------------

/// 输出合法的 Rust 字符串字面量（名称只含 ASCII 标识符，但仍做完整转义）。
fn rust_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render_table(out: &mut String, decl: &str, entries: &[(u32, String)]) {
    out.push_str(decl);
    out.push_str(" = &[\n");
    for (id, name) in entries {
        out.push_str(&format!("    ({id}, {}),\n", rust_string(name)));
    }
    out.push_str("];\n\n");
}

fn render(
    blocks: &[(u32, String)],
    block_states: &[(u32, String, u16)],
    items: &[(u32, String)],
    entities: &[(u32, String)],
    packets: &[(String, String, i32)],
) -> String {
    let mut out = String::new();
    out.push_str("// @generated by crates/mc-registry/build.rs — DO NOT EDIT.\n");
    out.push_str("// Source: data/manifest.json (26.3 / 777) + data/777/*.json\n");
    out.push_str("// blockstate IDs come from `default_state_id` in blocks.json.\n\n");

    render_table(&mut out, "pub static BLOCKS: &[(u32, &str)]", blocks);
    out.push_str("pub static BLOCK_STATES: &[(u32, &str, u16)] = &[\n");
    for (first, name, count) in block_states {
        out.push_str(&format!("    ({first}, {}, {count}),\n", rust_string(name)));
    }
    out.push_str("];\n\n");
    render_table(&mut out, "pub static ITEMS: &[(u32, &str)]", items);
    render_table(&mut out, "pub static ENTITIES: &[(u32, &str)]", entities);

    out.push_str("pub static PACKETS: &[(&str, &str, i32)] = &[\n");
    for (state, name, id) in packets {
        out.push_str(&format!(
            "    ({}, {}, {id}),\n",
            rust_string(state),
            rust_string(name)
        ));
    }
    out.push_str("];\n\n");

    out.push_str("pub fn by_name(name: &str) -> Option<u32> {\n    match name {\n");
    for (id, name) in blocks {
        out.push_str(&format!(
            "        {} => Some({id}),\n",
            rust_string(name)
        ));
    }
    out.push_str("        _ => None,\n    }\n}\n\n");

    out.push_str("pub fn by_id(id: u32) -> Option<&'static str> {\n    match id {\n");
    for (id, name) in blocks {
        out.push_str(&format!(
            "        {id} => Some({}),\n",
            rust_string(name)
        ));
    }
    out.push_str("        _ => None,\n    }\n}\n");

    out
}
