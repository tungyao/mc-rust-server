//! worldgen 单测：水/沙岸/出生点/树/biome/洞穴/矿/编码。
use super::*;
use mc_protocol::packets::{
    motion_blocking_heightmap, PlayChunkData, BEACH_BIOME_ID_777, FOREST_BIOME_ID_777,
    OCEAN_BIOME_ID_777, PLAINS_BIOME_ID_777,
};
use std::collections::HashMap;

// P44 新增测试用的额外导入（旧测试本体一字未改）。
use crate::crops::{BIRCH, DARK_OAK, OAK_TREE, SPRUCE};
use crate::{
    CLAY_BLOCK_ID, COARSE_DIRT_BLOCK_ID, COPPER_ORE_BLOCK_ID, CORNFLOWER_BLOCK_ID,
    DIAMOND_ORE_BLOCK_ID, EMERALD_ORE_BLOCK_ID, FERN_BLOCK_ID, GOLD_ORE_BLOCK_ID,
    GRAVEL_BLOCK_ID, ICE_BLOCK_ID, KELP_BLOCK_ID, LAPIS_ORE_BLOCK_ID, LILY_PAD_BLOCK_ID,
    POPPY_BLOCK_ID, PODZOL_BLOCK_ID, REDSTONE_ORE_BLOCK_ID, SANDSTONE_BLOCK_ID,
    SEAGRASS_BLOCK_ID, SHORT_GRASS_BLOCK_ID, SNOW_BLOCK_ID, TALL_GRASS_BLOCK_ID,
};

const WATER_SEED: i64 = -987654321;

fn block_at(g: &GeneratedChunk, lx: i32, y: i32, lz: i32) -> u16 {
    let section = ((y - MIN_Y) >> 4) as usize;
    let local_y = ((y - MIN_Y) & 15) as usize;
    g.sections[section][local_y * 256 + lz as usize * 16 + lx as usize]
}

fn find_column<F: Fn(i32) -> bool>(g: &GeneratedChunk, pred: F) -> Option<(i32, i32)> {
    for lz in 0..16 {
        for lx in 0..16 {
            let h = g.heights[(lz * 16 + lx) as usize];
            if pred(h) {
                return Some((lx, lz));
            }
        }
    }
    None
}

fn find_chunk_with_water(gen: &WorldGen, radius: i32) -> GeneratedChunk {
    let center = gen.spawn_chunk();
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let g = gen.generate(ChunkPos(center.0 + dx, center.1 + dz));
            if g.heights.iter().any(|&h| h < SEA_LEVEL) {
                return g;
            }
        }
    }
    panic!("no chunk with water within radius {radius} of spawn");
}

/// 逐 section 解析 network_bytes（paletted container 自定界），用于核对计数/biome。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SectionInfo {
    block_count: u16,
    fluid_count: u16,
    biome: i32,
}

fn read_varint_at(buf: &[u8], off: &mut usize) -> i32 {
    let (v, n) = mc_protocol::read_varint(&buf[*off..]).unwrap();
    *off += n;
    v
}

fn parse_sections(bytes: &[u8]) -> Vec<SectionInfo> {
    let mut out = Vec::with_capacity(SECTION_COUNT);
    let mut off = 0usize;
    for _ in 0..SECTION_COUNT {
        let block_count = u16::from_be_bytes([bytes[off], bytes[off + 1]]);
        let fluid_count = u16::from_be_bytes([bytes[off + 2], bytes[off + 3]]);
        off += 4;
        let bpe = bytes[off];
        off += 1;
        if bpe == 0 {
            read_varint_at(bytes, &mut off);
        } else {
            if bpe < 15 {
                let palette_len = read_varint_at(bytes, &mut off);
                for _ in 0..palette_len {
                    read_varint_at(bytes, &mut off);
                }
            }
            let per_long = 64 / bpe as usize;
            let longs = SECTION_VOLUME.div_ceil(per_long);
            off += longs * 8;
        }
        let biome_bpe = bytes[off];
        assert_eq!(biome_bpe, 0, "biome is single-valued per section");
        off += 1;
        let biome = read_varint_at(bytes, &mut off);
        out.push(SectionInfo { block_count, fluid_count, biome });
    }
    assert_eq!(off, bytes.len(), "section parser must consume the whole Data field");
    out
}

#[test]
fn same_seed_same_chunk_is_identical() {
    let gen = WorldGen::new(DEFAULT_SEED);
    let a = gen.generate(ChunkPos(23, 23));
    let b = gen.generate(ChunkPos(23, 23));
    assert_eq!(a.heights, b.heights);
    assert_eq!(a.motion_heights, b.motion_heights);
    assert_eq!(a.biome_id, b.biome_id);
    assert_eq!(a.sections, b.sections);
    // 跨实例同 seed 也一致。
    let c = WorldGen::new(DEFAULT_SEED).generate(ChunkPos(23, 23));
    assert_eq!(a.sections, c.sections);
    assert_eq!(a.motion_heights, c.motion_heights);
    assert_eq!(a.biome_id, c.biome_id);
}

#[test]
fn different_seed_changes_terrain() {
    let a = WorldGen::new(DEFAULT_SEED);
    let b = WorldGen::new(42);
    let mut diff = 0;
    for x in (320..=420).step_by(4) {
        for z in (330..=430).step_by(4) {
            if a.height_at(x, z) != b.height_at(x, z) {
                diff += 1;
            }
        }
    }
    assert!(diff > 20, "different seeds must produce visibly different terrain, diff={diff}");
    let g1 = a.generate(ChunkPos(23, 23));
    let g2 = b.generate(ChunkPos(23, 23));
    assert_ne!(g1.heights, g2.heights);
    assert_ne!(g1.sections, g2.sections);
}

#[test]
fn heights_are_bounded_and_adjacent_difference_is_small() {
    let gen = WorldGen::new(987654321);
    let mut prev_row = vec![0i32; 97];
    for z in -48..=48 {
        let mut row = Vec::with_capacity(97);
        for x in -48..=48 {
            let h = gen.height_at(x, z);
            assert!((MIN_SURFACE_Y..=MAX_SURFACE_Y).contains(&h), "height {h} at {x},{z}");
            row.push(h);
        }
        for pair in row.windows(2) {
            assert!(
                (pair[0] - pair[1]).abs() <= 3,
                "adjacent columns differ too much: {pair:?}"
            );
        }
        if z > -48 {
            for (x, (&h, &prev)) in row.iter().zip(prev_row.iter()).enumerate() {
                assert!(
                    (h - prev).abs() <= 3,
                    "z-adjacent columns differ too much at x={}: {h} vs {prev}",
                    x as i32 - 48
                );
            }
        }
        prev_row = row;
    }
}

#[test]
fn spawn_protection_is_smooth_for_known_seeds() {
    // 出生点半径 16 的抬升不能造成 1 格墙：4 个 seed 的相邻高度差仍 ≤3。
    for seed in [DEFAULT_SEED, 987654321, 42, -987654321] {
        let gen = WorldGen::new(seed);
        let mut prev_row: Vec<i32> = Vec::new();
        for z in 365..=397 {
            let mut row = Vec::with_capacity(33);
            for x in 353..=385 {
                let h = gen.height_at(x, z);
                assert!((MIN_SURFACE_Y..=MAX_SURFACE_Y).contains(&h), "seed {seed} h={h} at {x},{z}");
                row.push(h);
            }
            for pair in row.windows(2) {
                assert!(
                    (pair[0] - pair[1]).abs() <= 3,
                    "seed {seed}: adjacent columns differ too much: {pair:?}"
                );
            }
            if !prev_row.is_empty() {
                for (x, (&h, &prev)) in row.iter().zip(prev_row.iter()).enumerate() {
                    assert!(
                        (h - prev).abs() <= 3,
                        "seed {seed}: z-adjacent columns differ too much at x={}: {h} vs {prev}",
                        x as i32 + 353
                    );
                }
            }
            prev_row = row;
        }
    }
}

#[test]
fn chunk_boundaries_and_generated_heights_are_continuous() {
    let gen = WorldGen::new(DEFAULT_SEED);
    let west = gen.generate(ChunkPos(0, 0));
    let east = gen.generate(ChunkPos(1, 0));
    for lz in 0..16 {
        for lx in 0..16 {
            let wx = lx;
            let wz = lz;
            assert_eq!(west.heights[(lz * 16 + lx) as usize], gen.height_at(wx, wz));
            assert_eq!(
                east.heights[(lz * 16 + lx) as usize],
                gen.height_at(16 + lx, wz),
                "generated height must match height_at in next chunk"
            );
        }
        // 跨 chunk 边界的相邻列（世界 x=15/16）。
        let a = gen.height_at(15, lz);
        let b = gen.height_at(16, lz);
        assert!((a - b).abs() <= 3, "chunk boundary step {a}->{b} at z={lz}");
    }
}

#[test]
fn spawn_is_dry_grass_and_clear_for_known_seeds() {
    for seed in [DEFAULT_SEED, 987654321, 42, -987654321] {
        let gen = WorldGen::new(seed);
        let spawn = gen.spawn_pos();
        assert_eq!((spawn.0, spawn.2), (SPAWN_BLOCK_X, SPAWN_BLOCK_Z), "seed {seed}");
        assert!(spawn.1 > SEA_LEVEL + 1, "seed {seed}: spawn y={} must be > {}", spawn.1, SEA_LEVEL + 1);
        assert_eq!(spawn.1, gen.height_at(SPAWN_BLOCK_X, SPAWN_BLOCK_Z) + 1);
        assert_eq!(gen.spawn_chunk(), ChunkPos(23, 23));
        let g = gen.generate(gen.spawn_chunk());
        let mut chunks: HashMap<ChunkPos, GeneratedChunk> = HashMap::new();
        chunks.insert(g.pos, g);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let wx = SPAWN_BLOCK_X + dx;
                let wz = SPAWN_BLOCK_Z + dz;
                let h = gen.height_at(wx, wz);
                assert!(h >= SEA_LEVEL + 2, "seed {seed}: spawn 3x3 surface {h} at {wx},{wz}");
                let pos = ChunkPos(wx.div_euclid(16), wz.div_euclid(16));
                let g = chunks.entry(pos).or_insert_with(|| gen.generate(pos));
                let lx = wx - pos.0 * 16;
                let lz = wz - pos.1 * 16;
                assert_eq!(g.heights[(lz * 16 + lx) as usize], h);
                assert_eq!(block_at(g, lx, h, lz), GRASS_BLOCK_ID, "seed {seed}: spawn feet must be grass");
                // 3x3 内没有水。
                for y in (h + 1)..=(SEA_LEVEL + 4) {
                    assert_ne!(block_at(g, lx, y, lz), WATER_BLOCK_ID, "seed {seed}: water at spawn 3x3");
                }
            }
        }
        // 出生点 4 格内不长树：任何高于地表的列都没有 log/leaves。
        for dz in -4..=4 {
            for dx in -4..=4 {
                let wx = SPAWN_BLOCK_X + dx;
                let wz = SPAWN_BLOCK_Z + dz;
                let pos = ChunkPos(wx.div_euclid(16), wz.div_euclid(16));
                let g = chunks.entry(pos).or_insert_with(|| gen.generate(pos));
                let lx = wx - pos.0 * 16;
                let lz = wz - pos.1 * 16;
                let h = g.heights[(lz * 16 + lx) as usize];
                for y in (h + 1)..=(h + 10) {
                    let b = block_at(g, lx, y, lz);
                    assert!(
                        b != OAK_LOG_BLOCK_ID && b != OAK_LEAVES_BLOCK_ID,
                        "seed {seed}: tree within 4 of spawn at {wx},{y},{wz}"
                    );
                }
            }
        }
        // 出生点脚下方块是草，头顶是空气，且没有洞穴贴脸。
        let lx = SPAWN_BLOCK_X - 23 * 16;
        let lz = SPAWN_BLOCK_Z - 23 * 16;
        let g = chunks.get(&ChunkPos(23, 23)).unwrap();
        assert_eq!(block_at(g, lx, spawn.1 - 1, lz), GRASS_BLOCK_ID);
        assert_eq!(block_at(g, lx, spawn.1, lz), AIR_BLOCK_ID);
    }
}

#[test]
fn layers_are_grass_dirt_stone_and_air_in_spawn_column() {
    let gen = WorldGen::new(DEFAULT_SEED);
    let g = gen.generate(ChunkPos(23, 23));
    let spawn = gen.spawn_pos();
    let lx = SPAWN_BLOCK_X - 23 * 16;
    let lz = SPAWN_BLOCK_Z - 23 * 16;
    let h = g.heights[(lz * 16 + lx) as usize];
    assert_eq!(h, spawn.1 - 1);
    assert!(h > SEA_LEVEL + 2);
    assert_eq!(block_at(&g, lx, h, lz), GRASS_BLOCK_ID);
    for d in 1..=3 {
        assert_eq!(block_at(&g, lx, h - d, lz), DIRT_BLOCK_ID, "dirt layer {d}");
    }
    assert_eq!(block_at(&g, lx, h - 4, lz), STONE_BLOCK_ID, "stone below dirt");
    assert_eq!(block_at(&g, lx, h + 1, lz), AIR_BLOCK_ID, "air above grass");
    assert_eq!(block_at(&g, lx, MIN_Y, lz), STONE_BLOCK_ID, "bedrock-ish floor is stone");
}

#[test]
fn sandy_and_underwater_columns_use_sand_and_water() {
    let gen = WorldGen::new(WATER_SEED);
    let g = find_chunk_with_water(&gen, 8);
    let (lx, lz) = find_column(&g, |h| h < SEA_LEVEL).expect("water column");
    let h = g.heights[(lz * 16 + lx) as usize];
    assert!(h <= SEA_LEVEL + 2);
    // 顶层 4 格全是沙。
    for d in 0..=3 {
        assert_eq!(
            block_at(&g, lx, h - d, lz),
            SAND_BLOCK_ID,
            "sand layer {d} at h={h}"
        );
    }
    assert_eq!(block_at(&g, lx, h - 4, lz), STONE_BLOCK_ID, "stone below sand");
    // 水面到 SEA_LEVEL，水下没有空气缺口。
    for y in (h + 1)..=SEA_LEVEL {
        assert_eq!(block_at(&g, lx, y, lz), WATER_BLOCK_ID, "water at y={y}");
    }
    assert_eq!(block_at(&g, lx, SEA_LEVEL + 1, lz), AIR_BLOCK_ID, "air above sea");
    // 靠岸的沙柱（h 在 SEA_LEVEL+1..=SEA_LEVEL+2）顶层也是沙。
    if let Some((sx, sz)) = find_column(&g, |h| (SEA_LEVEL + 1..=SEA_LEVEL + 2).contains(&h)) {
        let h = g.heights[(sz * 16 + sx) as usize];
        assert_eq!(block_at(&g, sx, h, sz), SAND_BLOCK_ID, "beach top is sand");
        assert_eq!(block_at(&g, sx, h - 1, sz), SAND_BLOCK_ID);
    }
}

#[test]
fn heightmap_and_light_follow_actual_columns() {
    let gen = WorldGen::new(WATER_SEED);
    let g = find_chunk_with_water(&gen, 8);
    // 逐列扫描最高非空气方块（土/沙/石/水/log/leaves 都算 MOTION_BLOCKING）。
    for lz in 0..16 {
        for lx in 0..16 {
            let h = g.heights[(lz * 16 + lx) as usize];
            let mut top = MIN_Y - 1;
            for y in MIN_Y..=(MAX_SURFACE_Y + 16) {
                if block_at(&g, lx, y, lz) != AIR_BLOCK_ID {
                    top = y;
                }
            }
            let idx = (lz * 16 + lx) as usize;
            assert_eq!(g.motion_heights[idx], top + 1 - MIN_Y, "motion height at lx={lx} lz={lz}");
            assert!(g.motion_heights[idx] >= h + 1 - MIN_Y);
        }
    }
    assert_eq!(
        g.heightmaps_bytes(),
        motion_blocking_heightmap(&g.motion_heights)
    );
    // 水面列：MOTION_BLOCKING = SEA_LEVEL+1-MIN_Y。
    let (wx, wz) = find_column(&g, |h| h < SEA_LEVEL).unwrap();
    assert_eq!(g.motion_heights[(wz * 16 + wx) as usize], SEA_LEVEL + 1 - MIN_Y);
    // 天空光仍从最低地表 section 向上。
    let min_h = *g.heights.iter().min().unwrap();
    let first_lit = ((min_h - MIN_Y) / 16) as u32;
    let mask = ((1u64 << SECTION_COUNT) - 1) & !((1u64 << first_lit) - 1);
    let mut expected = Vec::new();
    mc_protocol::packets::encode_sky_light_data(mask, 15, &mut expected);
    assert_eq!(g.light_bytes(), expected);
    assert!(g.light_bytes().len() > 6, "water column chunk must carry sky light arrays");
    assert!(!g.network_bytes().is_empty());
}

#[test]
fn tree_columns_have_logs_on_grass_and_leaves_above_surface() {
    let gen = WorldGen::new(DEFAULT_SEED);
    let center = gen.spawn_chunk();
    let mut trees = 0;
    for dx in -4..=4 {
        for dz in -4..=4 {
            let g = gen.generate(ChunkPos(center.0 + dx, center.1 + dz));
            for lz in 0..16 {
                for lx in 0..16 {
                    let column = (lz * 16 + lx) as usize;
                    let h = g.heights[column];
                    if block_at(&g, lx, h + 1, lz) != OAK_LOG_BLOCK_ID {
                        continue;
                    }
                    trees += 1;
                    // 树干连续，底部站在草方块上，不悬空、不穿水。
                    let mut trunk_top = h + 1;
                    while block_at(&g, lx, trunk_top + 1, lz) == OAK_LOG_BLOCK_ID {
                        trunk_top += 1;
                    }
                    for y in (h + 1)..=trunk_top {
                        assert_eq!(block_at(&g, lx, y, lz), OAK_LOG_BLOCK_ID, "continuous trunk at y={y}");
                    }
                    assert!((4..=6).contains(&(trunk_top - h)), "trunk height {}", trunk_top - h);
                    assert_eq!(block_at(&g, lx, h, lz), GRASS_BLOCK_ID, "trunk must stand on grass");
                    // 树冠在树干顶部上方，树顶高于地表。
                    assert_eq!(block_at(&g, lx, trunk_top + 1, lz), OAK_LEAVES_BLOCK_ID, "canopy cross above trunk");
                    let mut top = trunk_top;
                    for y in (trunk_top + 1)..=(MAX_SURFACE_Y + 8) {
                        if block_at(&g, lx, y, lz) != AIR_BLOCK_ID {
                            top = y;
                        }
                    }
                    assert!(top > h + 1, "tree top must be above the surface");
                    assert_eq!(g.motion_heights[column], top + 1 - MIN_Y);
                }
            }
        }
    }
    assert!(trees > 20, "expected many oak trees near spawn, found {trees}");
}

#[test]
fn tree_canopy_is_continuous_across_chunk_boundary() {
    let gen = WorldGen::new(DEFAULT_SEED);
    let pos = gen.spawn_chunk();
    let base_x = pos.0 * 16;
    let base_z = pos.1 * 16;
    // 找一棵树冠跨东边界的树（origin x 在 chunk 末 2..=5 列内）。
    let mut found = None;
    'outer: for wx in (base_x + 12)..=(base_x + 17) {
        for wz in (base_z + 1)..=(base_z + 14) {
            let h = gen.height_at(wx, wz);
            let slope_ok = (gen.height_at(wx - 1, wz) - h).abs() <= 1
                && (gen.height_at(wx + 1, wz) - h).abs() <= 1
                && (gen.height_at(wx, wz - 1) - h).abs() <= 1
                && (gen.height_at(wx, wz + 1) - h).abs() <= 1;
            if let Some(t) = gen.tree_for(wx, wz, h, slope_ok, gen.biome_at(wx, wz)) {
                if t.x + 2 > base_x + 15 {
                    found = Some(t);
                    break 'outer;
                }
            }
        }
    }
    let tree = found.expect("expected a boundary-crossing oak near spawn");
    let east = ChunkPos(pos.0 + 1, pos.1);
    let west = gen.generate(pos);
    let east_chunk = gen.generate(east);
    // 枚举树的所有方块，按世界坐标找到所属 chunk，验证两侧都已写入。
    let mut outside = 0;
    let mut check = |wx: i32, y: i32, wz: i32, want: &[u16]| {
        let owner = ChunkPos(wx.div_euclid(16), wz.div_euclid(16));
        let g = if owner == pos {
            &west
        } else {
            assert_eq!(owner, east, "tree must not span more than two chunks here");
            &east_chunk
        };
        if owner != pos {
            outside += 1;
        }
        let got = block_at(g, wx - owner.0 * 16, y, wz - owner.1 * 16);
        assert!(want.contains(&got), "tree block {wx},{y},{wz} is {got}, want {want:?}");
    };
    let top = tree.top_y();
    for y in (tree.ground_y + 1)..=top {
        check(tree.x, y, tree.z, &[OAK_LOG_BLOCK_ID]);
    }
    for dy in -2i32..=1 {
        let y = top + dy;
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                let shape = match dy {
                    -2 | -1 => !(dx.abs() == 2 && dz.abs() == 2),
                    0 => dx.abs() <= 1 && dz.abs() <= 1,
                    _ => dx.abs() + dz.abs() <= 1,
                };
                if shape {
                    check(
                        tree.x + dx,
                        y,
                        tree.z + dz,
                        &[OAK_LEAVES_BLOCK_ID, OAK_LOG_BLOCK_ID],
                    );
                }
            }
        }
    }
    assert!(outside > 0, "expected canopy blocks in the neighbouring chunk");
}

#[test]
fn caves_and_ores_exist_underground_but_keep_surface_shell() {
    let gen = WorldGen::new(DEFAULT_SEED);
    let center = gen.spawn_chunk();
    let mut cave = 0usize;
    let mut coal = 0usize;
    let mut iron = 0usize;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let g = gen.generate(ChunkPos(center.0 + dx, center.1 + dz));
            for lz in 0..16 {
                for lx in 0..16 {
                    let h = g.heights[(lz * 16 + lx) as usize];
                    // 地表 8 格壳（h-7..=h）不被洞穴破坏。
                    for y in (h - 7)..=h {
                        assert_ne!(block_at(&g, lx, y, lz), AIR_BLOCK_ID, "cave broke surface shell at {lx},{y},{lz}");
                    }
                    for y in (MIN_Y + 9)..=(h - 9) {
                        match block_at(&g, lx, y, lz) {
                            AIR_BLOCK_ID => cave += 1,
                            COAL_ORE_BLOCK_ID => {
                                coal += 1;
                                assert!((COAL_MIN_Y..=COAL_MAX_Y).contains(&y));
                            }
                            IRON_ORE_BLOCK_ID => {
                                iron += 1;
                                assert!((IRON_MIN_Y..=IRON_MAX_Y).contains(&y));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    assert!(cave > 100, "expected underground caves, found {cave} air blocks");
    assert!(coal > 20, "expected coal ore, found {coal}");
    assert!(iron > 10, "expected iron ore, found {iron}");
    // 默认 seed 的 spawn chunk 地表完整（shell 断言已覆盖），且同 seed 确定。
    let a = gen.generate(center);
    let b = WorldGen::new(DEFAULT_SEED).generate(center);
    assert_eq!(a.sections, b.sections);
}

#[test]
fn biome_selection_matches_terrain_and_all_sections() {
    let gen = WorldGen::new(WATER_SEED);
    let center = gen.spawn_chunk();
    let mut ocean = None;
    let mut beach = None;
    let mut forest = None;
    let mut plains = None;
    let mut water_chunk_bytes = None;
    for dx in -10..=10 {
        for dz in -10..=10 {
            let pos = ChunkPos(center.0 + dx, center.1 + dz);
            let biome = gen.chunk_biome(pos);
            let water = gen.chunk_water_sand(pos).0;
            if biome == OCEAN_BIOME_ID_777 && water >= BIOME_WATER_SAMPLES {
                ocean.get_or_insert(pos);
            }
            if biome == BEACH_BIOME_ID_777 {
                beach.get_or_insert(pos);
            }
            if biome == FOREST_BIOME_ID_777 {
                forest.get_or_insert(pos);
            }
            if biome == PLAINS_BIOME_ID_777 {
                plains.get_or_insert(pos);
            }
            if water_chunk_bytes.is_none() && water >= BIOME_WATER_SAMPLES {
                water_chunk_bytes = Some(pos);
            }
        }
    }
    let ocean = ocean.expect("expected an ocean biome chunk");
    let _beach = beach.expect("expected a beach biome chunk");
    let forest = forest.expect("expected a forest biome chunk");
    let _plains = plains.expect("expected a plains biome chunk");
    assert_eq!(gen.chunk_biome(ocean), OCEAN_BIOME_ID_777);
    // 出生 chunk 不应是 ocean/beach。
    let spawn_biome = gen.chunk_biome(center);
    assert!(spawn_biome == PLAINS_BIOME_ID_777 || spawn_biome == FOREST_BIOME_ID_777);

    // 水 chunk：所有 section 的 biome 都等于 chunk.biome_id，fluid_count 有值。
    let g = gen.generate(water_chunk_bytes.unwrap());
    let infos = parse_sections(&g.network_bytes());
    assert!(infos.iter().any(|s| s.fluid_count > 0), "water chunk must report fluids");
    for (i, info) in infos.iter().enumerate() {
        assert_eq!(info.biome, g.biome_id, "section {i} biome");
        let section = &g.sections[i];
        let non_air = section.iter().filter(|&&b| b != AIR_BLOCK_ID).count();
        let fluid = section.iter().filter(|&&b| b == WATER_BLOCK_ID).count();
        assert_eq!(info.block_count as usize, non_air, "section {i} block_count");
        assert_eq!(info.fluid_count as usize, fluid, "section {i} fluid_count");
    }
    // forest chunk 的树密度高于 plains（同一 16×16 内树原点更多）。
    let density = |pos: ChunkPos| {
        let g = gen.generate(pos);
        let mut trunks = 0;
        for lz in 0..16 {
            for lx in 0..16 {
                let h = g.heights[(lz * 16 + lx) as usize];
                if block_at(&g, lx, h + 1, lz) == OAK_LOG_BLOCK_ID {
                    trunks += 1;
                }
            }
        }
        trunks
    };
    let mut forest_trees = 0;
    for dx in -2..=2 {
        for dz in -2..=2 {
            let pos = ChunkPos(forest.0 + dx, forest.1 + dz);
            if gen.chunk_biome(pos) == FOREST_BIOME_ID_777 {
                forest_trees += density(pos);
            }
        }
    }
    assert!(forest_trees > 0, "forest chunks must contain trees");
}

#[test]
fn uniform_water_section_reports_4096_blocks_and_fluids() {
    // 合成一个整节全水：WorldGen 的均匀水分支必须写 block_count=fluid_count=4096
    // 且 biome 用 chunk.biome_id（ocean=36）。
    let mut sections: Vec<Box<[u16; 4096]>> = (0..SECTION_COUNT)
        .map(|_| Box::new([AIR_BLOCK_ID; 4096]))
        .collect();
    sections[3] = Box::new([WATER_BLOCK_ID; 4096]);
    let g = GeneratedChunk {
        pos: ChunkPos(0, 0),
        sections,
        heights: [SEA_LEVEL; 256],
        motion_heights: [SEA_LEVEL + 1 - MIN_Y; 256],
        biome_id: OCEAN_BIOME_ID_777,
    };
    let infos = parse_sections(&g.network_bytes());
    assert_eq!(infos[3].block_count, 4096);
    assert_eq!(infos[3].fluid_count, 4096);
    assert_eq!(infos[3].biome, OCEAN_BIOME_ID_777);
    for (i, info) in infos.iter().enumerate() {
        assert!(info.fluid_count == 0 || i == 3);
    }
}

#[test]
fn network_bytes_decode_as_play_chunk_data() {
    let gen = WorldGen::new(WATER_SEED);
    let g = find_chunk_with_water(&gen, 8);
    let data = PlayChunkData {
        chunk_x: g.pos.0,
        chunk_z: g.pos.1,
        heightmaps_nbt: g.heightmaps_bytes(),
        section_bytes: g.network_bytes(),
        light_bytes: g.light_bytes(),
    };
    let mut body = Vec::new();
    data.encode_body(&mut body);
    let (decoded, used) = PlayChunkData::decode_body(&body).unwrap();
    assert_eq!(used, body.len());
    assert_eq!(decoded, data);
    // 高度图仍是 1 个 MOTION_BLOCKING + 37 longs。
    assert_eq!(&data.heightmaps_nbt[..3], &[0x01, 0x04, 0x25]);
}

#[test]
fn negative_and_far_coordinates_are_deterministic() {
    let gen = WorldGen::new(-987654321);
    for pos in [ChunkPos(-1, -1), ChunkPos(-40, 33), ChunkPos(1_800_000, -900_000)] {
        let a = gen.generate(pos);
        let b = gen.generate(pos);
        assert_eq!(a.heights, b.heights);
        assert_eq!(a.motion_heights, b.motion_heights);
        assert_eq!(a.biome_id, b.biome_id);
        assert_eq!(a.sections, b.sections);
        assert!(a.heights.iter().all(|h| (MIN_SURFACE_Y..=MAX_SURFACE_Y).contains(h)));
    }
}

#[test]
fn default_seed_matches_server_default() {
    assert_eq!(DEFAULT_SEED, 123456789);
    assert_eq!(WorldGen::default(), WorldGen::new(123456789));
    // 固定回归：默认种子出生点地表 72 → 站立 y=73；另一个种子 y 不同。
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    assert_eq!(spawn, BlockPos(369, 73, 381));
    assert_ne!(
        spawn.1,
        WorldGen::new(987654321).spawn_pos().1,
        "different seed must produce a different spawn y"
    );
}

// ---------------------------------------------------------------------------
// P45：结构性地貌 / 多 biome / 逐列天空光。以下全部是新增测试；
// 上面所有 P16 与 P44 测试一字未改。
// ---------------------------------------------------------------------------

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn chunk_digest(g: &GeneratedChunk) -> u64 {
    let mut h = fnv1a64(&g.heights.iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<_>>());
    h ^= fnv1a64(&g.motion_heights.iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<_>>());
    h ^= fnv1a64(&g.network_bytes());
    h ^= fnv1a64(&g.heightmaps_bytes());
    h ^= fnv1a64(&g.light_bytes());
    h
}

/// v1 是 golden：P1 之后任何基底/编码改动都会让这些哈希漂移。
#[test]
fn v1_is_frozen_byte_for_byte() {
    for ((x, z), want) in [
        ((23, 23), 0xb88a629b884d4457u64),
        ((0, 0), 0xbff1225f680a85d7u64),
        ((-1, -1), 0xf3b0397a3dc1a783u64),
        ((1_800_000, -900_000), 0xeae777396d266b85u64),
    ] {
        let g = WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V1)
            .generate(ChunkPos(x, z));
        assert_eq!(chunk_digest(&g), want, "v1 golden drift at {x},{z}");
    }
    // 默认构造器始终是 v1。
    let pos = ChunkPos(23, 23);
    assert_eq!(
        WorldGen::new(DEFAULT_SEED).generate(pos).sections,
        WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V1).generate(pos).sections
    );
    assert_eq!(WorldGen::default().version, GeneratorVersion::V1);
}

/// v2 矿种高度带：每种新矿在 3×3 chunk 里都出现，且 y 严格落在声明带内。
#[test]
fn v2_new_ores_appear_in_documented_height_bands() {
    let gen = WorldGen::with_version(42, GeneratorVersion::V2);
    let center = gen.spawn_chunk();
    let mut counts = HashMap::new();
    for dx in -1..=1 {
        for dz in -1..=1 {
            let g = gen.generate(ChunkPos(center.0 + dx, center.1 + dz));
            for lz in 0..16 {
                for lx in 0..16 {
                    let h = g.heights[(lz * 16 + lx) as usize];
                    for y in (MIN_Y + 9)..=(h - 9) {
                        let id = block_at(&g, lx, y, lz);
                        if let Some(band) = ores::band_for_id(id) {
                            assert!(
                                (band.min_y..=band.max_y).contains(&y),
                                "ore {} at y={y} outside {}..={}",
                                band.id, band.min_y, band.max_y
                            );
                            *counts.entry(id).or_insert(0usize) += 1;
                        }
                    }
                }
            }
        }
    }
    for id in [
        COAL_ORE_BLOCK_ID,
        IRON_ORE_BLOCK_ID,
        COPPER_ORE_BLOCK_ID,
        GOLD_ORE_BLOCK_ID,
        REDSTONE_ORE_BLOCK_ID,
        LAPIS_ORE_BLOCK_ID,
        DIAMOND_ORE_BLOCK_ID,
        EMERALD_ORE_BLOCK_ID,
    ] {
        assert!(counts.get(&id).copied().unwrap_or(0) > 0, "ore {id} never generated in v2");
    }
}

/// v2 树种来自 `crops::TREE_SPECS` 的真实 log/leaves ID，且树冠跨 chunk 连续。
#[test]
fn v2_multiple_tree_species_and_cross_chunk_canopy_continuity() {
    use std::collections::HashSet;
    let gen = WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V2);
    let center = gen.spawn_chunk();
    let mut species = HashSet::new();
    let logs = [
        OAK_TREE.log_id,
        BIRCH.log_id,
        SPRUCE.log_id,
        DARK_OAK.log_id,
    ];
    for dx in -2..=2 {
        for dz in -2..=2 {
            let g = gen.generate(ChunkPos(center.0 + dx, center.1 + dz));
            for lz in 0..16 {
                for lx in 0..16 {
                    let h = g.heights[(lz * 16 + lx) as usize];
                    for y in (h + 1)..=(h + 12) {
                        let id = block_at(&g, lx, y, lz);
                        if logs.contains(&id) {
                            species.insert(id);
                        }
                    }
                }
            }
        }
    }
    assert!(species.len() >= 2, "expected multiple tree species, got {species:?}");

    // 找一棵树冠跨东边界的候选树，验证两侧 chunk 都写到了它的方块。
    let base_x = center.0 * 16;
    let base_z = center.1 * 16;
    let mut found = None;
    'outer: for wx in (base_x + 12)..=(base_x + 17) {
        for wz in (base_z + 3)..=(base_z + 12) {
            let h = gen.height_at(wx, wz);
            let slope_ok = (gen.height_at(wx - 1, wz) - h).abs() <= 1
                && (gen.height_at(wx + 1, wz) - h).abs() <= 1
                && (gen.height_at(wx, wz - 1) - h).abs() <= 1
                && (gen.height_at(wx, wz + 1) - h).abs() <= 1;
            if let Some(t) = features::tree_for(&gen, wx, wz, h, slope_ok, gen.biome_at(wx, wz)) {
                if t.x + 2 > base_x + 15 {
                    found = Some(t);
                    break 'outer;
                }
            }
        }
    }
    let tree = found.expect("expected a boundary-crossing v2 tree near spawn");
    let west = gen.generate(center);
    let east = gen.generate(ChunkPos(center.0 + 1, center.1));
    let mut outside = 0;
    let mut check = |wx: i32, y: i32, wz: i32, want: &[u16]| {
        let owner = ChunkPos(wx.div_euclid(16), wz.div_euclid(16));
        let g = if owner == center { &west } else { &east };
        if owner != center {
            outside += 1;
        }
        let got = block_at(g, wx - owner.0 * 16, y, wz - owner.1 * 16);
        assert!(want.contains(&got), "v2 tree block {wx},{y},{wz} is {got}, want {want:?}");
    };
    // 邻树的树冠可能与本树重叠，所以接受任意树种的 log/leaves（只验证本树
    // 的方块确实写进了两侧 chunk，而非把重叠冠形误判为错误）。
    let tree_blocks = [
        OAK_TREE.log_id,
        OAK_TREE.leaves_id,
        BIRCH.log_id,
        BIRCH.leaves_id,
        SPRUCE.log_id,
        SPRUCE.leaves_id,
        DARK_OAK.log_id,
        DARK_OAK.leaves_id,
    ];
    let top = tree.top_y();
    for y in (tree.ground_y + 1)..=top {
        check(tree.x, y, tree.z, &tree_blocks);
    }
    for dy in -2i32..=1 {
        let y = top + dy;
        let radius: i32 = if tree.spec.name == "spruce" {
            match dy {
                -2 => 1,
                -1 => 2,
                0 => 1,
                _ => 0,
            }
        } else {
            2
        };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let shape = if tree.spec.name == "spruce" {
                    true
                } else {
                    match dy {
                        -2 | -1 => !(dx.abs() == 2 && dz.abs() == 2),
                        0 => dx.abs() <= 1 && dz.abs() <= 1,
                        _ => dx.abs() + dz.abs() <= 1,
                    }
                };
                if shape {
                    check(tree.x + dx, y, tree.z + dz, &tree_blocks);
                }
            }
        }
    }
    assert!(outside > 0, "expected canopy blocks in the neighbouring chunk");
}

/// v2 地表过渡 / 草花 / 静态水岸植被：新方块确实被生成，且 ID 来自查表。
#[test]
fn v2_surface_materials_and_plants_are_generated() {
    // 直接判定高地材质分支与冰封分支（避免依赖具体 seed 的地形分布）。
    let high = features::surface_layers(DEFAULT_SEED, 100, 100, 90, FOREST_BIOME_ID_777);
    assert!(
        high.iter().any(|&id| id == SNOW_BLOCK_ID || id == COARSE_DIRT_BLOCK_ID || id == STONE_BLOCK_ID),
        "high ground must use transition materials: {high:?}"
    );
    let mut ice = false;
    for x in 0..512 {
        if features::water_at(DEFAULT_SEED, x, 300, SEA_LEVEL - 1) == ICE_BLOCK_ID {
            ice = true;
            break;
        }
    }
    assert!(ice, "shallow water must be able to freeze in v2");

    let gen = WorldGen::with_version(WATER_SEED, GeneratorVersion::V2);
    let center = gen.spawn_chunk();
    let mut found = std::collections::HashSet::new();
    for dx in -8..=8 {
        for dz in -8..=8 {
            let g = gen.generate(ChunkPos(center.0 + dx, center.1 + dz));
            for section in &g.sections {
                for &id in section.iter() {
                    found.insert(id);
                }
            }
        }
    }
    for id in [
        GRAVEL_BLOCK_ID,
        SANDSTONE_BLOCK_ID,
        COARSE_DIRT_BLOCK_ID,
        PODZOL_BLOCK_ID,
        CLAY_BLOCK_ID,
        ICE_BLOCK_ID,
        SHORT_GRASS_BLOCK_ID,
        TALL_GRASS_BLOCK_ID,
        FERN_BLOCK_ID,
        POPPY_BLOCK_ID,
        CORNFLOWER_BLOCK_ID,
        SEAGRASS_BLOCK_ID,
        KELP_BLOCK_ID,
        LILY_PAD_BLOCK_ID,
    ] {
        assert!(found.contains(&id), "v2 block {id} never generated near water seed");
    }
    assert!(is_plant(SHORT_GRASS_BLOCK_ID));
    assert!(!is_plant(STONE_BLOCK_ID));
}

/// v2 纯函数性：同 seed 同块逐字节一致；高度场与出生点保持 v1 冻结口径；
/// 出生圈内不出现新树/新植物，3×3 仍是干燥草地。
#[test]
fn v2_is_pure_deterministic_and_spawn_stays_safe() {
    let gen = WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V2);
    let pos = ChunkPos(23, 23);
    let a = gen.generate(pos);
    let b = WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V2).generate(pos);
    assert_eq!(a.sections, b.sections);
    assert_eq!(a.heights, b.heights);
    assert_eq!(a.motion_heights, b.motion_heights);
    assert_eq!(a.network_bytes(), b.network_bytes());
    assert_eq!(a.heights, WorldGen::new(DEFAULT_SEED).generate(pos).heights, "height field must be frozen");
    assert_eq!(gen.spawn_pos(), WorldGen::new(DEFAULT_SEED).spawn_pos());

    for seed in [DEFAULT_SEED, 42, -987654321] {
        let gen = WorldGen::with_version(seed, GeneratorVersion::V2);
        let center = gen.spawn_chunk();
        let spawn = gen.spawn_pos();
        assert_eq!(spawn.1, WorldGen::new(seed).spawn_pos().1, "spawn y must not change");
        let g = gen.generate(center);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let wx = SPAWN_BLOCK_X + dx;
                let wz = SPAWN_BLOCK_Z + dz;
                let owner = ChunkPos(wx.div_euclid(16), wz.div_euclid(16));
                let owned;
                let chunk = if owner == center { &g } else { owned = gen.generate(owner); &owned };
                let lx = wx - owner.0 * 16;
                let lz = wz - owner.1 * 16;
                let h = chunk.heights[(lz * 16 + lx) as usize];
                assert_eq!(h, gen.height_at(wx, wz));
                assert!(h > SEA_LEVEL + 1, "spawn 3x3 must stay dry for seed {seed}");
                assert_eq!(block_at(chunk, lx, h, lz), GRASS_BLOCK_ID, "spawn 3x3 grass");
            }
        }
        // 出生点 4 格内没有树/草花（树候选排除半径 7 + 冠半径 2 = 最近 6 格），
        // 脚下是草，头顶是空气。
        let lx = SPAWN_BLOCK_X - center.0 * 16;
        let lz = SPAWN_BLOCK_Z - center.1 * 16;
        let h = g.heights[(lz * 16 + lx) as usize];
        assert_eq!(block_at(&g, lx, h, lz), GRASS_BLOCK_ID);
        assert_eq!(block_at(&g, lx, h + 1, lz), AIR_BLOCK_ID);
        for dz in -4..=4 {
            for dx in -4..=4 {
                let wx = SPAWN_BLOCK_X + dx;
                let wz = SPAWN_BLOCK_Z + dz;
                let owner = ChunkPos(wx.div_euclid(16), wz.div_euclid(16));
                let owned;
                let chunk = if owner == center { &g } else { owned = gen.generate(owner); &owned };
                let bx = wx - owner.0 * 16;
                let bz = wz - owner.1 * 16;
                let bh = chunk.heights[(bz * 16 + bx) as usize];
                for y in (bh + 1)..=(bh + 10) {
                    let id = block_at(chunk, bx, y, bz);
                    assert!(
                        id != OAK_LOG_BLOCK_ID
                            && id != OAK_LEAVES_BLOCK_ID
                            && id != BIRCH.log_id
                            && id != BIRCH.leaves_id
                            && id != SPRUCE.log_id
                            && id != SPRUCE.leaves_id
                            && !is_plant(id),
                        "v2 vegetation inside spawn 4 for seed {seed} at {wx},{y},{wz}: {id}"
                    );
                }
            }
        }
    }
}

// ---- P45 §1：洞穴仍保 8 格壳、且不再是 v1 的整格方块 ----

#[test]
fn p45_v2_caves_keep_surface_shell_and_are_not_v1_blocky() {
    let gen = WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V2);
    let old = WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V1);
    let center = gen.spawn_chunk();
    let mut cave = 0usize;
    let mut changed = 0usize;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let pos = ChunkPos(center.0 + dx, center.1 + dz);
            let g = gen.generate(pos);
            let g1 = old.generate(pos);
            for lz in 0..16 {
                for lx in 0..16 {
                    let h = g.heights[(lz * 16 + lx) as usize];
                    for y in (h - 7)..=h {
                        assert_ne!(block_at(&g, lx, y, lz), AIR_BLOCK_ID, "v2 cave broke shell at {lx},{y},{lz}");
                    }
                    for y in (MIN_Y + 9)..=(h - 9) {
                        let a = block_at(&g, lx, y, lz);
                        let b = block_at(&g1, lx, y, lz);
                        if a == AIR_BLOCK_ID {
                            cave += 1;
                        }
                        if (a == AIR_BLOCK_ID) != (b == AIR_BLOCK_ID) {
                            changed += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(cave > 100, "expected v2 underground caves, found {cave}");
    assert!(changed > 100, "v2 interpolation must change the v1 whole-cell pattern, changed {changed}");
}

// ---- P45 §4：64 项 biome 容器逐字节解码 ----

#[derive(Debug, Clone, PartialEq, Eq)]
struct BiomeSectionInfo {
    block_count: u16,
    fluid_count: u16,
    biomes: Vec<i32>, // 64 项
}

/// 按 wiki 的 4×4×4 biome paletted container 自定界解析（与编码器独立的手写解码）。
fn parse_biome_sections(bytes: &[u8]) -> Vec<BiomeSectionInfo> {
    let mut out = Vec::with_capacity(SECTION_COUNT);
    let mut off = 0usize;
    for _ in 0..SECTION_COUNT {
        let block_count = u16::from_be_bytes([bytes[off], bytes[off + 1]]);
        let fluid_count = u16::from_be_bytes([bytes[off + 2], bytes[off + 3]]);
        off += 4;
        let bpe = bytes[off] as usize;
        off += 1;
        if bpe == 0 {
            read_varint_at(bytes, &mut off); // block 单值
        } else {
            if bpe < 15 {
                let palette_len = read_varint_at(bytes, &mut off);
                for _ in 0..palette_len {
                    read_varint_at(bytes, &mut off);
                }
            }
            off += SECTION_VOLUME.div_ceil(64 / bpe) * 8;
        }
        let biome_bpe = bytes[off] as usize;
        off += 1;
        let mut biomes = vec![0i32; 64];
        if biome_bpe == 0 {
            biomes.fill(read_varint_at(bytes, &mut off));
        } else {
            let (palette, direct) = if biome_bpe <= 3 {
                let n = read_varint_at(bytes, &mut off) as usize;
                let p: Vec<i32> = (0..n).map(|_| read_varint_at(bytes, &mut off)).collect();
                (p, false)
            } else {
                assert_eq!(biome_bpe, 7, "biome indirect BPE must be <=3, direct is 7");
                (Vec::new(), true)
            };
            let per_long = 64 / biome_bpe;
            let longs = 64usize.div_ceil(per_long);
            for li in 0..longs {
                let word = u64::from_be_bytes(bytes[off..off + 8].try_into().unwrap());
                off += 8;
                for k in 0..per_long {
                    let idx = li * per_long + k;
                    if idx >= 64 {
                        break;
                    }
                    let v = ((word >> (k * biome_bpe)) & ((1u64 << biome_bpe) - 1)) as usize;
                    biomes[idx] = if direct { v as i32 } else { palette[v] };
                }
            }
        }
        out.push(BiomeSectionInfo { block_count, fluid_count, biomes });
    }
    assert_eq!(off, bytes.len(), "v2 section parser must consume the whole Data field");
    out
}

#[test]
fn p45_v2_biome_container_decodes_byte_for_byte() {
    let gen = WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V2);
    let pos = gen.spawn_chunk();
    let g = gen.generate(pos);
    let bytes = g.network_bytes_v2(&gen);
    let sections = parse_biome_sections(&bytes);
    assert_eq!(sections.len(), SECTION_COUNT);
    let mut multi = 0usize;
    for (i, info) in sections.iter().enumerate() {
        let section = &g.sections[i];
        let non_air = section.iter().filter(|&&b| b != AIR_BLOCK_ID).count();
        let fluid = section.iter().filter(|&&b| b == WATER_BLOCK_ID).count();
        assert_eq!(info.block_count as usize, non_air, "section {i} block_count");
        assert_eq!(info.fluid_count as usize, fluid, "section {i} fluid_count");
        assert_eq!(info.biomes, biome::biomes_for_section(&gen, pos, i), "section {i} biomes");
        if info.biomes.iter().any(|&b| b != info.biomes[0]) {
            multi += 1;
        }
    }
    assert!(multi >= 5, "expected multiple non-uniform biome sections, got {multi}");

    // 0x2E 包体 roundtrip（Data 字段内部是 64 项 biome 容器）。
    let data = PlayChunkData {
        chunk_x: pos.0,
        chunk_z: pos.1,
        heightmaps_nbt: g.heightmaps_bytes(),
        section_bytes: bytes,
        light_bytes: g.light_bytes_v2(),
    };
    let mut body = Vec::new();
    data.encode_body(&mut body);
    let (got, used) = PlayChunkData::decode_body(&body).unwrap();
    assert_eq!(used, body.len());
    assert_eq!(got, data);
}

// ---- P45 §5：逐列天空光 nibble 逐字节解码 ----

fn light_nibble(arr: &[u8; 2048], lx: i32, y: i32, lz: i32) -> u8 {
    let local_y = ((y - MIN_Y) & 15) as usize;
    let idx = local_y * 256 + lz as usize * 16 + lx as usize;
    if idx % 2 == 0 { arr[idx / 2] & 0x0f } else { arr[idx / 2] >> 4 }
}

#[test]
fn p45_v2_light_data_decodes_byte_for_byte() {
    let gen = WorldGen::with_version(WATER_SEED, GeneratorVersion::V2);
    // 出生附近找一条 v2 河道作为水下光照样本。
    let center = gen.spawn_chunk();
    let mut target = None;
    'outer: for dx in -12..=12 {
        for dz in -12..=12 {
            let pos = ChunkPos(center.0 + dx, center.1 + dz);
            let g = gen.generate(pos);
            for lz in 0..16 {
                for lx in 0..16 {
                    let h = g.heights[(lz * 16 + lx) as usize];
                    // 排除顶层被冰封的柱（冰不透明，水下会是 0）。
                    if h < SEA_LEVEL - 2 && block_at(&g, lx, SEA_LEVEL, lz) == WATER_BLOCK_ID {
                        target = Some((g, lx, lz));
                        break 'outer;
                    }
                }
            }
        }
    }
    let (g, water_x, water_z) = target.expect("expected a v2 water column near spawn");
    let bytes = g.light_bytes_v2();
    let mut off = 0usize;
    let mut masks = [[0u8; 8]; 4];
    for mask in &mut masks {
        let (n, used) = mc_protocol::read_varint(&bytes[off..]).unwrap();
        off += used;
        assert!(n >= 0 && n as usize <= 8);
        mask[..n as usize].copy_from_slice(&bytes[off..off + n as usize]);
        off += n as usize;
    }
    assert_eq!(masks[1], [0u8; 8], "block mask empty");
    assert_eq!(masks[2], [0u8; 8], "empty sky mask empty");
    assert_eq!(masks[3], [0u8; 8], "empty block mask empty");
    let (count, used) = mc_protocol::read_varint(&bytes[off..]).unwrap();
    off += used;
    assert!(count > 0);
    let expected = light::compute_sky_light(&g);
    assert_eq!(count as usize, expected.len());
    for (section, nibble) in &expected {
        assert!(masks[0][*section as usize / 8] & (1 << (*section as usize % 8)) != 0, "section {section} in mask");
        let (len, used) = mc_protocol::read_varint(&bytes[off..]).unwrap();
        off += used;
        assert_eq!(len, 2048, "light array length VarInt");
        assert_eq!(&bytes[off..off + 2048], nibble.as_slice(), "section {section} nibble");
        off += 2048;
    }
    let (block_arrays, used) = mc_protocol::read_varint(&bytes[off..]).unwrap();
    off += used;
    assert_eq!(block_arrays, 0);
    assert_eq!(off, bytes.len(), "light parser must consume all bytes");

    // 天空起点逐列：水面以下 nibble 非 15，地表以上为 15，洞穴地下为 0。
    let sea_section = ((SEA_LEVEL - MIN_Y) >> 4) as usize;
    let sea_nibble = &expected.iter().find(|(s, _)| *s as usize == sea_section).unwrap().1;
    assert_eq!(light_nibble(sea_nibble, water_x, SEA_LEVEL - 1, water_z), 13, "water attenuates sky light");
    let top = expected.last().unwrap();
    assert!(top.1.iter().all(|&b| b == 0xff), "top section fully lit");
    assert!(expected.iter().all(|(s, _)| *s >= 7), "underground sections carry no light");
}

// ---- P45 §2/§5：seed 一致性与远坐标 ----

#[test]
fn p45_v2_is_pure_and_far_coordinates_are_deterministic() {
    let gen = WorldGen::with_version(-987654321, GeneratorVersion::V2);
    for pos in [ChunkPos(-1, -1), ChunkPos(-40, 33), ChunkPos(1_800_000, -900_000), gen.spawn_chunk()] {
        let a = gen.generate(pos);
        let b = WorldGen::with_version(-987654321, GeneratorVersion::V2).generate(pos);
        assert_eq!(a.sections, b.sections);
        assert_eq!(a.heights, b.heights);
        assert_eq!(a.motion_heights, b.motion_heights);
        assert_eq!(a.network_bytes_v2(&gen), b.network_bytes_v2(&gen));
        assert_eq!(a.light_bytes_v2(), b.light_bytes_v2());
        assert!(a.heights.iter().all(|h| (MIN_SURFACE_Y..=MAX_SURFACE_Y).contains(h)));
        // 包体必须能被 0x2E 解码器完整消费。
        let data = PlayChunkData {
            chunk_x: pos.0,
            chunk_z: pos.1,
            heightmaps_nbt: a.heightmaps_bytes(),
            section_bytes: a.network_bytes_v2(&gen),
            light_bytes: a.light_bytes_v2(),
        };
        let mut body = Vec::new();
        data.encode_body(&mut body);
        let (got, used) = PlayChunkData::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, data);
    }
}

// ---- P45 §5/§2.5：441 块 Join 估算 ----

#[test]
fn p45_v2_441_chunk_join_budget_estimate() {
    use rayon::prelude::*;
    use std::time::Instant;
    let positions: Vec<ChunkPos> = (-10..=10)
        .flat_map(|dz| (-10..=10).map(move |dx| ChunkPos(23 + dx, 23 + dz)))
        .collect();
    assert_eq!(positions.len(), 441);
    let measure = |version: GeneratorVersion, seed: i64| {
        let gen = WorldGen::with_version(seed, version);
        let start = Instant::now();
        let total: usize = positions
            .par_iter()
            .map(|&pos| {
                let g = gen.generate(pos);
                let data = PlayChunkData {
                    chunk_x: pos.0,
                    chunk_z: pos.1,
                    heightmaps_nbt: g.heightmaps_bytes(),
                    section_bytes: match version {
                        GeneratorVersion::V1 => g.network_bytes(),
                        GeneratorVersion::V2 => g.network_bytes_v2(&gen),
                    },
                    light_bytes: match version {
                        GeneratorVersion::V1 => g.light_bytes(),
                        GeneratorVersion::V2 => g.light_bytes_v2(),
                    },
                };
                let mut body = Vec::new();
                data.encode_body(&mut body);
                body.len()
            })
            .sum();
        (start.elapsed(), total / positions.len())
    };
    let (v1_time, v1_avg) = measure(GeneratorVersion::V1, 42);
    let (v2_time, v2_avg) = measure(GeneratorVersion::V2, 42);
    println!(
        "441 chunk generate+encode: v1={v1_time:?} ({v1_avg}B/chunk), v2={v2_time:?} ({v2_avg}B/chunk)"
    );
    // 真机基线 496–558ms、红线 700ms；本环境没有原版客户端，只做本地估算：
    // ① 同时量 v1/v2 作为相对参照，避免 debug 机器绝对耗时造成 flaky；
    // ② 同时给出单块平均生成+编码耗时（真机红色目标 ≤~12ms/块）。
    let per_chunk_ms = v2_time.as_secs_f64() * 1000.0 / positions.len() as f64;
    println!("v2 per-chunk generate+encode: {per_chunk_ms:.3}ms (parallel wall / 441)");
    assert!(
        v2_time <= v1_time * 2 + std::time::Duration::from_millis(100),
        "v2 generate+encode too slow: v1={v1_time:?} v2={v2_time:?}"
    );
    assert!(v2_avg < 60 * 1024, "v2 body {v2_avg}B must stay near the 60KB budget");
}
