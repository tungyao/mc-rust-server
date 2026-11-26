//! crops: 网络侧作物专属逻辑的集中模块（P37 拆分）。
//!
//! 只放**纯判定/路由**与挖掘副作用编排：手持 → 作物路由、种植/骨粉判定、
//! 挖掘连带摘除与竖向连根。socket 写包、背包扣减、经验球、后台逻辑仍在
//! `mod.rs` / `drop_sync.rs` 中，不进入本模块（薄 shim 留在 `block_edit.rs`）。

use super::block_edit::{
    eye_block_dist, in_view, rollback, BlockChange, BlockEditOutcome,
};
use crate::drop_state::DropState;
use mc_protocol::packets;
use mc_world::crops::{
    age_crop_of, age_stage_id, cocoa_of, cocoa_state, fruit_neighbors, soil_accepts,
    tree_spec_for_sapling, vertical_spec_of, AgeCropSpec, PlantKind, Soil, TreeSpec,
    VerticalSpec, COCOA, PITCHER_CROP,
};
use mc_world::MutableWorld;

/// 手持物品 → 种植路由。`Legacy` 保留 P22/P23/P33 的旧入口（含橡树苗），
/// 其余分支走 P37 新判定，互不影响。
#[derive(Debug, Clone, Copy)]
pub enum HeldPlant {
    Legacy(u16),
    Age(&'static AgeCropSpec),
    Tree(&'static TreeSpec),
    Vertical(&'static VerticalSpec),
    Cocoa,
    None,
}

/// 手持 → 作物（旧表 + P37 新表），单一入口，供 `play_loop` 调用。
pub fn held_plant(item_id: i32) -> HeldPlant {
    if let Some(id) = Legacy::for_item(item_id) {
        return HeldPlant::Legacy(id);
    }
    if item_id == COCOA.seed_item {
        return HeldPlant::Cocoa;
    }
    for spec in &mc_world::crops::AGE_CROPS {
        if spec.seed_item == item_id {
            return HeldPlant::Age(spec);
        }
    }
    for spec in &mc_world::crops::TREE_SPECS {
        if spec.seed_item == item_id {
            return HeldPlant::Tree(spec);
        }
    }
    for spec in &mc_world::crops::VERTICAL_SPECS {
        if spec.item_id == item_id {
            return HeldPlant::Vertical(spec);
        }
    }
    HeldPlant::None
}

struct Legacy;

impl Legacy {
    fn for_item(item_id: i32) -> Option<u16> {
        if item_id == mc_world::drops::ITEM_OAK_SAPLING {
            return Some(mc_world::OAK_SAPLING_BLOCK_ID);
        }
        mc_world::crop_base_for_seed(item_id)
    }
}

/// 顶面种植判定（统一土壤类型）。旧 `decide_plant` 以 `Soil::Crop` 调用，
/// 语义逐字节不变；新作物按各自土壤传入（灵魂沙/沙子等）。
pub fn decide_plant_top(
    world: Option<&MutableWorld>,
    eye: (f64, f64, f64),
    center: mc_core::ChunkPos,
    radius: i32,
    act: &packets::UseItemOn,
    plant_id: u16,
    soil: Soil,
) -> Option<BlockEditOutcome> {
    let world = world?;
    if act.face != 1 {
        return Some(BlockEditOutcome { sequence: act.sequence, changes: Vec::new(), drops: Vec::new() });
    }
    let (tx, ty, tz) = (act.x, act.y + 1, act.z);
    if !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&act.y)
        || !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&ty)
        || !in_view(center, radius, tx, tz)
        || eye_block_dist(eye, act.x, act.y, act.z) > mc_world::MAX_REACH
    {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![rollback(world, tx, ty, tz)],
            drops: Vec::new(),
        });
    }
    let clicked = world.get(act.x, act.y, act.z);
    if !soil_accepts(soil, clicked) {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![rollback(world, tx, ty, tz)],
            drops: Vec::new(),
        });
    }
    let cur = world.get(tx, ty, tz);
    if cur != mc_world::AIR_BLOCK_ID {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![BlockChange { x: tx, y: ty, z: tz, old_id: cur, new_id: cur }],
            drops: Vec::new(),
        });
    }
    let old = world.set(tx, ty, tz, plant_id);
    Some(BlockEditOutcome {
        sequence: act.sequence,
        changes: vec![BlockChange { x: tx, y: ty, z: tz, old_id: old, new_id: plant_id }],
        drops: Vec::new(),
    })
}

/// 侧面种植（可可豆）：只允许 face 2..5，被点必须是 jungle_log，
/// 目标格空气；y/视距/距离口径与顶面种植完全一致。
pub fn decide_plant_side(
    world: Option<&MutableWorld>,
    eye: (f64, f64, f64),
    center: mc_core::ChunkPos,
    radius: i32,
    act: &packets::UseItemOn,
) -> Option<BlockEditOutcome> {
    let world = world?;
    let Some(facing) = mc_world::crops::cocoa_facing_index(act.face) else {
        return Some(BlockEditOutcome { sequence: act.sequence, changes: Vec::new(), drops: Vec::new() });
    };
    let (dx, dz) = match facing {
        0 => (0, -1),
        1 => (0, 1),
        2 => (-1, 0),
        _ => (1, 0),
    };
    let (tx, ty, tz) = (act.x + dx, act.y, act.z + dz);
    if !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&act.y)
        || !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&ty)
        || !in_view(center, radius, tx, tz)
        || eye_block_dist(eye, act.x, act.y, act.z) > mc_world::MAX_REACH
    {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![rollback(world, tx, ty, tz)],
            drops: Vec::new(),
        });
    }
    if world.get(act.x, act.y, act.z) != COCOA.support_id {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![rollback(world, tx, ty, tz)],
            drops: Vec::new(),
        });
    }
    let cur = world.get(tx, ty, tz);
    if cur != mc_world::AIR_BLOCK_ID {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![BlockChange { x: tx, y: ty, z: tz, old_id: cur, new_id: cur }],
            drops: Vec::new(),
        });
    }
    let new_id = cocoa_state(0, facing);
    let old = world.set(tx, ty, tz, new_id);
    Some(BlockEditOutcome {
        sequence: act.sequence,
        changes: vec![BlockChange { x: tx, y: ty, z: tz, old_id: old, new_id }],
        drops: Vec::new(),
    })
}

/// 骨粉判定的中间结果（与 `block_edit::BonemealAction` 一一映射）。
#[derive(Debug, Clone)]
pub enum BonemealDecision {
    Advance(BlockEditOutcome),
    GrowOak,
    GrowSpecies(&'static TreeSpec),
    Noop,
}

fn single_change(world: &MutableWorld, sequence: i32, x: i32, y: i32, z: i32, new_id: u16) -> BlockEditOutcome {
    let old = world.set(x, y, z, new_id);
    BlockEditOutcome {
        sequence,
        changes: vec![BlockChange { x, y, z, old_id: old, new_id }],
        drops: Vec::new(),
    }
}

/// 竖著作物当前所在高度（从下往上数，0=基部）；状态里可能带 leaves/stage 变体。
pub fn vertical_height(world: &MutableWorld, spec: &VerticalSpec, x: i32, y: i32, z: i32) -> Option<u8> {
    if vertical_spec_of(world.get(x, y, z)).map(|s| s.name) != Some(spec.name) {
        return None;
    }
    let mut h = 0u8;
    while h < spec.max_height {
        let id = world.get(x, y - h as i32, z);
        if vertical_spec_of(id).map(|s| s.name) != Some(spec.name) {
            break;
        }
        h += 1;
    }
    Some(h - 1)
}

/// 骨粉作物判定（不含 y/视距/距离，已由 `block_edit::decide_bonemeal` 校验）。
/// 旧 P27/P33 口径保持不变：未熟推进 1~2 阶段、成熟 Noop、橡苗长树。
pub fn decide_bonemeal_crop(
    world: &MutableWorld,
    (x, y, z): (i32, i32, i32),
    sequence: i32,
) -> Option<BonemealDecision> {
    let clicked = world.get(x, y, z);
    if clicked == mc_world::OAK_SAPLING_BLOCK_ID {
        return Some(BonemealDecision::GrowOak);
    }
    if let Some(spec) = tree_spec_for_sapling(clicked) {
        return Some(BonemealDecision::GrowSpecies(spec));
    }
    if let Some((crop, age)) = mc_world::crop_age_of(clicked) {
        if age >= crop.max_age {
            return Some(BonemealDecision::Noop);
        }
        let adv = mc_world::drops::bonemeal_advance(world.seed(), x, y, z);
        let new_age = age.saturating_add(adv).min(crop.max_age);
        return Some(BonemealDecision::Advance(single_change(
            world, sequence, x, y, z, mc_world::crop_stage_id(crop, new_age),
        )));
    }
    if let Some((spec, age)) = age_crop_of(clicked) {
        if age >= spec.max_age {
            return Some(BonemealDecision::Noop);
        }
        let adv = mc_world::drops::bonemeal_advance(world.seed(), x, y, z);
        let new_age = age.saturating_add(adv).min(spec.max_age);
        let new_id = age_stage_id(spec, new_age);
        if spec.name == PITCHER_CROP.name {
            // 猪笼草上下两半都更新；上半只在空气时补（不覆盖玩家建筑）。
            let (lower, upper) = mc_world::crops::pitcher_states(new_age);
            let old_lower = world.set(x, y, z, lower);
            let mut changes = vec![BlockChange { x, y, z, old_id: old_lower, new_id: lower }];
            if world.get(x, y + 1, z) == mc_world::AIR_BLOCK_ID {
                let old_upper = world.set(x, y + 1, z, upper);
                changes.push(BlockChange { x, y: y + 1, z, old_id: old_upper, new_id: upper });
            }
            return Some(BonemealDecision::Advance(BlockEditOutcome {
                sequence,
                changes,
                drops: Vec::new(),
            }));
        }
        return Some(BonemealDecision::Advance(single_change(world, sequence, x, y, z, new_id)));
    }
    if let Some((_, age, facing)) = cocoa_of(clicked) {
        if age >= COCOA.max_age {
            return Some(BonemealDecision::Noop);
        }
        let adv = mc_world::drops::bonemeal_advance(world.seed(), x, y, z);
        let new_age = age.saturating_add(adv).min(COCOA.max_age);
        return Some(BonemealDecision::Advance(single_change(
            world, sequence, x, y, z, cocoa_state(new_age, facing),
        )));
    }
    if let Some(spec) = vertical_spec_of(clicked) {
        let Some(height) = vertical_height(world, spec, x, y, z) else {
            return Some(BonemealDecision::Noop);
        };
        let (bx, by, bz) = vertical_base(world, x, y, z);
        // 无论点中哪一段，新一段都落在整株顶端。
        let (tx, ty, tz) = (bx, by + height as i32 + 1, bz);
        if height >= spec.max_height - 1 || world.get(tx, ty, tz) != mc_world::AIR_BLOCK_ID {
            return Some(BonemealDecision::Noop);
        }
        let new_id = (spec.state_fn)(height + 1);
        return Some(BonemealDecision::Advance(single_change(world, sequence, tx, ty, tz, new_id)));
    }
    Some(BonemealDecision::Noop)
}

/// 挖掘副作用（P27 旧连带 + P37 新连带），在发包/生成掉落前就地改 `outcome`。
/// 旧方块（P33 作物/橡树苗/橡木原木）的判定顺序与旧实现一致，保证旧 e2e 逐字节不变。
pub fn apply_dig_effects(
    world: &MutableWorld,
    ds: &DropState,
    outcome: &mut BlockEditOutcome,
) {
    let snapshot = outcome.changes.clone();
    for c in snapshot {
        if c.new_id != mc_world::AIR_BLOCK_ID {
            continue;
        }
        // ---- P27 旧口径：草/泥土/耕地被挖，上方 P33 作物/橡树苗一起 pop ----
        if mc_world::is_crop_soil(c.old_id) {
            let above = world.get(c.x, c.y + 1, c.z);
            if mc_world::crop_age_of(above).is_some() {
                pop_above(world, ds, outcome, c.x, c.y, c.z, above, true);
            } else if above == mc_world::OAK_SAPLING_BLOCK_ID {
                pop_above(world, ds, outcome, c.x, c.y, c.z, above, false);
            } else if mc_world::plant_at(above).is_some() {
                // P37：新植物（含灵魂沙上作物之外的普通土作物）同样连带。
                pop_above(world, ds, outcome, c.x, c.y, c.z, above, false);
            }
        }
        // ---- P37：新土壤（灵魂沙/沙）被挖，上方新作物一起 pop ----
        if (c.old_id == mc_world::crops::SOUL_SAND_BLOCK_ID || c.old_id == mc_world::SAND_BLOCK_ID)
            && mc_world::plant_at(world.get(c.x, c.y + 1, c.z)).is_some()
        {
            let above = world.get(c.x, c.y + 1, c.z);
            pop_above(world, ds, outcome, c.x, c.y, c.z, above, false);
        }
        // ---- 猪笼草上下两半：挖任一半都连带另一半（age 模型之外的高低两格）----
        if let Some((spec, age)) = age_crop_of(c.old_id) {
            if spec.name == mc_world::crops::PITCHER_CROP.name {
                let (lower, upper) = mc_world::crops::pitcher_states(age);
                let (other, other_y) = if c.old_id == lower { (upper, c.y + 1) } else { (lower, c.y - 1) };
                ds.uproot_plant(c.x, if c.old_id == lower { c.y } else { other_y }, c.z);
                if world.get(c.x, other_y, c.z) == other {
                    let old = world.set(c.x, other_y, c.z, mc_world::AIR_BLOCK_ID);
                    outcome.changes.push(BlockChange {
                        x: c.x,
                        y: other_y,
                        z: c.z,
                        old_id: old,
                        new_id: mc_world::AIR_BLOCK_ID,
                    });
                    outcome.drops.extend(mc_world::drops_for_block(old, c.x, other_y, c.z, world.seed()));
                }
                continue;
            }
        }
        // ---- 直接挖新植物：摘除跟踪；竖著作物连根拔起 ----
        if let Some((kind, _)) = mc_world::plant_at(c.old_id) {
            let base = if matches!(kind, PlantKind::Vertical(_)) {
                vertical_base(world, c.x, c.y, c.z)
            } else {
                (c.x, c.y, c.z)
            };
            ds.uproot_plant(base.0, base.1, base.2);
            if let Some(spec) = vertical_spec_of(c.old_id) {
                collapse_vertical(world, ds, outcome, base, spec);
            }
        }
        // ---- 原木被挖：登记树叶 decay 观察（橡树=P27 旧口径，新树种同链路）----
        if c.old_id == mc_world::OAK_LOG_BLOCK_ID
            || mc_world::crops::tree_spec_for_log(c.old_id).is_some()
        {
            ds.schedule_leaf_decay_around(world, c.x, c.y, c.z);
        }
    }
}

fn pop_above(
    world: &MutableWorld,
    ds: &DropState,
    outcome: &mut BlockEditOutcome,
    x: i32,
    y: i32,
    z: i32,
    above: u16,
    is_crop: bool,
) {
    let old = world.set(x, y + 1, z, mc_world::AIR_BLOCK_ID);
    outcome.changes.push(BlockChange {
        x,
        y: y + 1,
        z,
        old_id: old,
        new_id: mc_world::AIR_BLOCK_ID,
    });
    outcome.drops.extend(mc_world::drops_for_block(old, x, y + 1, z, world.seed()));
    if is_crop {
        ds.uproot_crop(x, y + 1, z);
    } else if above == mc_world::OAK_SAPLING_BLOCK_ID {
        ds.uproot_sapling(x, y + 1, z);
    } else {
        ds.uproot_plant(x, y + 1, z);
    }
    if let Some(spec) = vertical_spec_of(above) {
        collapse_vertical(world, ds, outcome, (x, y + 1, z), spec);
    }
}

/// 从竖著作物任一段向下找到基部坐标。
pub fn vertical_base(world: &MutableWorld, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    let mut base_y = y;
    while vertical_spec_of(world.get(x, base_y - 1, z)).is_some() {
        base_y -= 1;
    }
    (x, base_y, z)
}

/// 竖著作物下方被挖：上方各段依次变空气并各自掉落（原版同链路）。
fn collapse_vertical(
    world: &MutableWorld,
    ds: &DropState,
    outcome: &mut BlockEditOutcome,
    (x, y, z): (i32, i32, i32),
    spec: &'static VerticalSpec,
) {
    let mut h = 1u8;
    while h < spec.max_height {
        let id = world.get(x, y + h as i32, z);
        if vertical_spec_of(id).map(|s| s.name) != Some(spec.name) {
            break;
        }
        let old = world.set(x, y + h as i32, z, mc_world::AIR_BLOCK_ID);
        outcome.changes.push(BlockChange {
            x,
            y: y + h as i32,
            z,
            old_id: old,
            new_id: mc_world::AIR_BLOCK_ID,
        });
        outcome.drops.extend(mc_world::drops_for_block(old, x, y + h as i32, z, world.seed()));
        h += 1;
    }
    let _ = ds; // 跟踪已在调用方按基部摘除
}

/// 瓜梗成熟后第一个可替换邻格（果实落点）；由 ticker 调用，纯查询。
pub fn fruit_target(world: &MutableWorld, spec: &AgeCropSpec, x: i32, y: i32, z: i32) -> Option<(i32, i32, i32)> {
    let fruit = spec.fruit?;
    let _ = fruit.block_id;
    fruit_neighbors(x, y, z)
        .into_iter()
        .find(|p| mc_world::is_replaceable(world.get(p.0, p.1, p.2)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_world::{WorldGen, DEFAULT_SEED};

    fn fixture() -> (MutableWorld, (f64, f64, f64), mc_core::ChunkPos) {
        let world = MutableWorld::new(DEFAULT_SEED);
        let s = WorldGen::new(DEFAULT_SEED).spawn_pos();
        let feet = (s.0 as f64 + 0.5, s.1 as f64, s.2 as f64 + 0.5);
        (world, feet, mc_core::ChunkPos(s.0.div_euclid(16), s.2.div_euclid(16)))
    }

    #[test]
    fn held_plant_routes_all_families() {
        use mc_world::crops::{BIRCH, BAMBOO, NETHER_WART, CARROT, COCOA};
        assert!(matches!(held_plant(mc_world::drops::ITEM_WHEAT_SEEDS), HeldPlant::Legacy(_)));
        assert!(matches!(held_plant(mc_world::drops::ITEM_OAK_SAPLING), HeldPlant::Legacy(_)));
        assert!(matches!(held_plant(CARROT.seed_item), HeldPlant::Legacy(_)));
        assert!(matches!(held_plant(NETHER_WART.seed_item), HeldPlant::Age(_)));
        assert!(matches!(held_plant(BIRCH.seed_item), HeldPlant::Tree(_)));
        assert!(matches!(held_plant(BAMBOO.item_id), HeldPlant::Vertical(_)));
        assert!(matches!(held_plant(COCOA.seed_item), HeldPlant::Cocoa));
        assert!(matches!(held_plant(mc_world::drops::ITEM_STONE), HeldPlant::None));
    }

    #[test]
    fn p37_soul_sand_and_side_planting_reject_old_soil_and_wrong_face() {
        let (world, feet, center) = fixture();
        let s = WorldGen::new(DEFAULT_SEED).spawn_pos();
        let (x, y, z) = (s.0, s.1, s.2);
        world.set(x, y, z, mc_world::crops::SOUL_SAND_BLOCK_ID);
        let act = |face| packets::UseItemOn {
            hand: 0, x, y, z, face, cursor_x: 0.5, cursor_y: 1.0, cursor_z: 0.5,
            inside_block: false, world_border_hit: false, sequence: 1,
        };
        let out = decide_plant_top(
            Some(&world), super::super::block_edit::eye_of(feet), center, 2, &act(1),
            mc_world::crops::NETHER_WART.base_id, Soil::SoulSand,
        )
        .unwrap();
        assert_eq!(out.changes[0].new_id, mc_world::crops::NETHER_WART.base_id);
        // 普通草土不支持地狱疣。
        world.set(x, y, z, mc_world::GRASS_BLOCK_ID);
        let out = decide_plant_top(
            Some(&world), super::super::block_edit::eye_of(feet), center, 2, &act(1),
            mc_world::crops::NETHER_WART.base_id, Soil::SoulSand,
        )
        .unwrap();
        assert_eq!(out.changes[0].old_id, out.changes[0].new_id);
        // 可可只贴 jungle_log 侧面。
        world.set(x, y, z, COCOA.support_id);
        let out = decide_plant_side(
            Some(&world), super::super::block_edit::eye_of(feet), center, 2, &act(1),
        )
        .unwrap();
        assert!(out.changes.is_empty(), "top face must not plant cocoa");
        let out = decide_plant_side(
            Some(&world), super::super::block_edit::eye_of(feet), center, 2, &act(3),
        )
        .unwrap();
        assert_eq!(out.changes[0].new_id, cocoa_state(0, 1));
    }

    #[test]
    fn vertical_height_and_bonemeal_decisions() {
        let world = MutableWorld::new(7);
        let spec = &mc_world::crops::SUGAR_CANE;
        world.set(0, 100, 0, spec.base_id);
        world.set(0, 101, 0, spec.base_id);
        assert_eq!(vertical_height(&world, spec, 0, 101, 0), Some(1));
        assert_eq!(vertical_height(&world, spec, 0, 100, 0), Some(0));
        match decide_bonemeal_crop(&world, (0, 101, 0), 4).unwrap() {
            BonemealDecision::Advance(out) => {
                assert_eq!(out.changes[0].new_id, spec.base_id);
                assert_eq!(out.changes[0].y, 102);
            }
            other => panic!("unripe vertical must grow, got {other:?}"),
        }
        world.set(0, 102, 0, spec.base_id);
        assert!(matches!(decide_bonemeal_crop(&world, (0, 100, 0), 4), Some(BonemealDecision::Noop)));
    }
}
