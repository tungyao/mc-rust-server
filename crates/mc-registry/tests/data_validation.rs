//! P0 校验：data/777/*.json 里的 ID 必须与真机实测值逐项一致。
//! 同时校验精简表与 full/ 原始资产、以及 build.rs 生成表之间没有漂移。
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn parse(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn data_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("data")
}

fn version_dir() -> PathBuf {
    let manifest = parse(&data_root().join("manifest.json"));
    assert_eq!(manifest["version"].as_str(), Some("26.3"));
    assert_eq!(manifest["protocol"].as_u64(), Some(777));
    assert_eq!(manifest["data_version"].as_u64(), Some(5023));
    let dir = manifest["data_dir"].as_str().expect("data_dir");
    data_root().join(dir)
}

fn block_default_state_id(blocks: &Value, name: &str) -> u64 {
    blocks["blocks"]
        .as_array()
        .expect("blocks array")
        .iter()
        .find(|b| b["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("block {name} missing"))
        ["default_state_id"]
        .as_u64()
        .expect("default_state_id")
}

fn item_id(items: &Value, name: &str) -> u64 {
    items[name]["id"].as_u64().unwrap_or_else(|| panic!("item {name} missing"))
}

fn packet_id(packets: &Value, side: &str, state: &str, name: &str) -> i64 {
    packets[side][state][name]
        .as_i64()
        .unwrap_or_else(|| panic!("packet {side}/{state}/{name} missing"))
}

#[test]
fn manifest_points_to_26_3_data() {
    let dir = version_dir();
    assert!(dir.is_dir(), "missing {}", dir.display());
    for name in ["blocks.json", "items.json", "entities.json", "packets.json", "properties.json"] {
        assert!(dir.join(name).is_file(), "missing {}/{name}", dir.display());
    }
    assert!(dir.join("SOURCE.md").is_file());
}

#[test]
fn block_default_state_ids_match_hardware() {
    let blocks = parse(&version_dir().join("blocks.json"));
    assert_eq!(blocks["blocks"].as_array().unwrap().len(), 1286);
    for (name, expected) in [
        ("air", 0),
        ("stone", 1),
        ("grass_block", 9),
        ("dirt", 10),
        ("water", 89),
        ("sand", 121),
        ("oak_log", 140),
        ("oak_leaves", 294),
        ("coal_ore", 136),
        ("iron_ore", 134),
    ] {
        assert_eq!(
            block_default_state_id(&blocks, name),
            expected,
            "blockstate ID drift: {name}"
        );
    }
    // 回归：grass_block 的 `id` 是 8，真值是 `default_state_id` = 9。
    let grass = blocks["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"].as_str() == Some("grass_block"))
        .unwrap();
    assert_eq!(grass["id"].as_u64(), Some(8));
    assert_eq!(grass["default_state_id"].as_u64(), Some(9));
}

#[test]
fn item_ids_match_hardware() {
    let items = parse(&version_dir().join("items.json"));
    assert_eq!(items.as_object().unwrap().len(), 1658);
    for (name, expected) in [
        ("stone", 1),
        ("dirt", 55),
        ("cobblestone", 62),
        ("oak_sapling", 77),
        ("sand", 88),
        ("oak_log", 163),
        ("coal", 1010),
        ("stick", 1060),
        ("raw_iron", 1017),
    ] {
        assert_eq!(item_id(&items, name), expected, "item ID drift: {name}");
    }
}

#[test]
fn packet_ids_match_hardware() {
    let packets = parse(&version_dir().join("packets.json"));
    assert_eq!(packets["version"].as_u64(), Some(777));
    for (side, name, expected) in [
        ("clientbound", "add_entity", 1),
        ("clientbound", "block_changed_ack", 4),
        ("clientbound", "block_update", 8),
        ("clientbound", "level_chunk_with_light", 0x2E),
        ("clientbound", "entity_position_sync", 0x23),
        ("clientbound", "set_entity_data", 0x65),
        ("clientbound", "take_item_entity", 0x7F),
    ] {
        assert_eq!(packet_id(&packets, side, "play", name), expected);
    }
    for (name, expected) in [
        ("player_action", 0x29),
        ("use_item_on", 0x42),
        ("move_player_pos", 0x1E),
    ] {
        assert_eq!(packet_id(&packets, "serverbound", "play", name), expected);
    }
}

#[test]
fn trimmed_tables_match_full_assets() {
    let dir = version_dir();

    let full_blocks = parse(&dir.join("full/blocks.json"));
    let blocks = parse(&dir.join("blocks.json"));
    for b in blocks["blocks"].as_array().unwrap() {
        let name = b["name"].as_str().unwrap();
        let full = full_blocks["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("full asset missing block {name}"));
        assert_eq!(b["default_state_id"], full["default_state_id"], "block {name}");
        assert_eq!(b["id"], full["id"], "block kind id {name}");
        assert_eq!(b["hardness"], full["hardness"], "block hardness {name}");
    }

    let full_items = parse(&dir.join("full/items.json"));
    let items = parse(&dir.join("items.json"));
    for (name, entry) in items.as_object().unwrap() {
        assert_eq!(entry["id"], full_items[name]["id"], "item {name}");
    }

    let full_entities = parse(&dir.join("full/entities.json"));
    let entities = parse(&dir.join("entities.json"));
    for (name, entry) in entities.as_object().unwrap() {
        assert_eq!(entry["id"], full_entities[name]["id"], "entity {name}");
    }
}

#[test]
fn generated_tables_match_json_data() {
    let dir = version_dir();

    let blocks = parse(&dir.join("blocks.json"));
    let expected: HashSet<(u32, &str)> = blocks["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["default_state_id"].as_u64().unwrap() as u32,
                b["name"].as_str().unwrap(),
            )
        })
        .collect();
    let actual: HashSet<(u32, &str)> = mc_registry::BLOCKS
        .iter()
        .map(|(id, name)| (*id, name.strip_prefix("minecraft:").unwrap_or(name)))
        .collect();
    assert_eq!(actual, expected, "generated BLOCKS drifted from blocks.json");

    for (file, table, kind) in [
        ("items.json", mc_registry::ITEMS, "items"),
        ("entities.json", mc_registry::ENTITIES, "entities"),
    ] {
        let json = parse(&dir.join(file));
        let expected: HashSet<(u32, &str)> = json
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, v)| (v["id"].as_u64().unwrap() as u32, name.as_str()))
            .collect();
        let actual: HashSet<(u32, &str)> = table
            .iter()
            .map(|(id, name)| (*id, name.strip_prefix("minecraft:").unwrap_or(name)))
            .collect();
        assert_eq!(actual, expected, "generated {kind} drifted from {file}");
    }

    let packets = parse(&dir.join("packets.json"));
    let mut expected = Vec::new();
    for side in ["serverbound", "clientbound"] {
        for (state, map) in packets[side].as_object().unwrap() {
            for (name, id) in map.as_object().unwrap() {
                expected.push((format!("{side}.{state}"), name.clone(), id.as_i64().unwrap() as i32));
            }
        }
    }
    expected.sort();
    let mut actual: Vec<(String, String, i32)> = mc_registry::PACKETS
        .iter()
        .map(|(s, n, id)| ((*s).to_owned(), (*n).to_owned(), *id))
        .collect();
    actual.sort();
    assert_eq!(actual, expected, "generated PACKETS drifted from packets.json");
}
