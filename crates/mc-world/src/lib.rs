//! mc-world: Chunk 存储 + 生成 + 存档，不依赖网络
//! 对标 FerrumC world + SteelMC parity 思路
pub mod crafting;
pub mod crops;
pub mod drops;
pub mod fluid;
pub mod load;
pub mod survival;
pub mod mutable;
pub mod save;
pub mod store; // P36：redb 自有世界存储原型（不接入生产路径，不做 Anvil）
pub mod worldgen;
pub mod items;
pub mod inventory;
pub mod tools;
pub mod uuid_migration; // P51：玩家存档 username → 稳定 UUID 键迁移
pub mod world_time; // P54 A6：全局时间/天气服务端状态
pub mod xp;

pub use crops::{
    any_age_crop_of, crop_age, crop_age_of, crop_base_for_seed, crop_for_seed, crop_stage_id,
    plant_at, CropKind, CropSpec, CROPS,
};
pub use drops::{Drop, block_for_item, drops_for_block};
pub use fluid::{
    is_water, swim_state, water_level, water_source_id, water_state_id, FlowRule, SwimState,
};
pub use survival::{SurvivalState, food_for_item};
pub use items::{ItemEntities, ItemEntity};
pub use inventory::PlayerInventory;
pub use xp::{XpOrb, XpOrbEntities};
pub use mutable::{
    MutableWorld, DEFAULT_PLACE_BLOCK_ID, EYE_HEIGHT, MAX_REACH, PLACEABLE_BLOCK_IDS,
    WORLD_TOP_Y, chunk_of_block, is_placeable, is_replaceable, with_journal_actor,
};
pub use worldgen::{
    GeneratedChunk, GeneratorVersion, WorldGen, DEFAULT_SEED, MAX_SURFACE_Y, MIN_SURFACE_Y,
    SEA_LEVEL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Uniform(u16),
    Paletted,
    Direct,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub pos: mc_core::ChunkPos,
    pub sections: Vec<SectionKind>, // 24 节：整节同值 Uniform，跨层混合 Paletted 占位
}

impl Chunk {
    pub fn empty_flat(pos: mc_core::ChunkPos) -> Self {
        Self { pos, sections: vec![SectionKind::Uniform(0); 24] }
    }
}

// ---------------------------------------------------------------------------
// 旧占位世界：Chunk/save/load 兼容 + 早期 Play 脚手架。
// P14 起 Play 网络路径使用 `worldgen::WorldGen`（seed 驱动、逐列高度）；
// 本文件的 `chunk_for`/`chunk_*_bytes` 仅保留给存档占位与旧测试。
// ---------------------------------------------------------------------------

/// 空世界种子占位（P3 接真实存档/配置）。确定性生成依赖它。
pub const WORLD_SEED: i64 = worldgen::DEFAULT_SEED;

/// 出生点 x/z（方块坐标）：保留原版抓包位置 (369,381) → chunk (23,23)。
/// y 不再写死，由 [`worldgen::WorldGen::spawn_pos`] 按 seed 计算为地表上方 1 格。
pub const SPAWN_BLOCK_X: i32 = 369;
pub const SPAWN_BLOCK_Z: i32 = 381;

/// 编译期 ID 查表：直接读 `mc-registry` 由 `build.rs` 从 `data/777/*.json` 生成的
/// 静态注册表，语义与 `mc_registry::block_id` / `mc_registry::item_id` 一致。
/// 这两个公开查询函数内部用 `format!` 做短名补全，不是 `const fn`；这里在生成的
/// 表上做等价的 const 线性查找，使 ID 既是编译期常量，又随数据 codegen 自动漂移。
/// 名称缺失时编译期 panic，不允许静默回退到旧的硬编码值。
const fn registry_id(table: &[(u32, &str)], name: &str) -> u32 {
    let needle = name.as_bytes();
    let mut i = 0;
    while i < table.len() {
        let (id, candidate) = table[i];
        let haystack = candidate.as_bytes();
        if haystack.len() == needle.len() {
            let mut j = 0;
            while j < needle.len() {
                if haystack[j] != needle[j] {
                    break;
                }
                j += 1;
            }
            if j == needle.len() {
                return id;
            }
        }
        i += 1;
    }
    panic!("mc-registry generated table is missing a required name");
}

/// P37 编译期 blockstate 解析：`full/blocks.json` 里每个方块的 states 连续排列，
/// 由 `mc-registry` 生成 `(first_state_id, name, count)`。按属性组合索引取 ID，
/// 避免 cocoa(age×facing)/bamboo(age×leaves×stage) 这类多属性方块写死数字。
const fn state_index_id(table: &[(u32, &str, u16)], name: &str, index: u16) -> u16 {
    let needle = name.as_bytes();
    let mut i = 0;
    while i < table.len() {
        let (first, candidate, count) = table[i];
        let haystack = candidate.as_bytes();
        let mut same = haystack.len() == needle.len();
        let mut j = 0;
        while same && j < needle.len() {
            same = haystack[j] == needle[j];
            j += 1;
        }
        if same {
            assert!(index < count, "blockstate index out of range");
            return (first + index as u32) as u16;
        }
        i += 1;
    }
    panic!("mc-registry generated block-state table is missing a required name");
}

/// 方块 ID 来自 `mc_registry::BLOCKS` 的 26.3 全局 blockstate ID（default_state_id）：
/// air=0、stone=1、grass_block（snowy=false）=9、dirt=10。
/// 这些 ID 也用于 Configuration 中回放的 `minecraft:block` 注册表。
/// 水/沙/树/矿 ID 同样来自该表（26.3 客户端 jar 实测值，见 `data/777/SOURCE.md`）。
pub const AIR_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:air") as u16;
pub const STONE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:stone") as u16;
/// `data/777/blocks.json` 的 `cobblestone.default_state_id=14`（P18 遗漏映射，
/// 管理页默认发放即圆石，右键只 ack 造成客户端预测幽灵块；P28 补上）。
pub const COBBLESTONE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:cobblestone") as u16;
pub const GRASS_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:grass_block") as u16;
pub const DIRT_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:dirt") as u16;
/// `Blocks.WATER.defaultBlockState()`（source，level=0）。
pub const WATER_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:water") as u16;
pub const SAND_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:sand") as u16;
/// `Blocks.OAK_LOG.defaultBlockState()`（axis=y）。
pub const OAK_LOG_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:oak_log") as u16;
pub const OAK_LEAVES_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:oak_leaves") as u16;
/// `Blocks.OAK_SAPLING.defaultBlockState()`（stage=0）：`data/777/blocks.json` 的
/// `oak_sapling.default_state_id=30`（P23 种植用；长成树/骨粉留后续）。
pub const OAK_SAPLING_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:oak_sapling") as u16;
pub const COAL_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:coal_ore") as u16;
pub const IRON_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:iron_ore") as u16;
/// `Blocks.FARMLAND.defaultBlockState()`（干燥 moisture=0）：`data/777/blocks.json` 的
/// `farmland.default_state_id=6875`；P27 只用这一个默认态（无水分/踩回退化，见 HANDOFF §5）。
pub const FARMLAND_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:farmland") as u16;
// ---------------------------------------------------------------------------
// P44 v2 worldgen 新方块：全部从 `data/777/blocks.json` 的 default_state_id 查表，
// 不手写数字；v1 基底完全不使用这些 ID。
// ---------------------------------------------------------------------------
/// 地表过渡材质。
pub const GRAVEL_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:gravel") as u16;
pub const SANDSTONE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:sandstone") as u16;
pub const COARSE_DIRT_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:coarse_dirt") as u16;
pub const PODZOL_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:podzol") as u16;
pub const CLAY_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:clay") as u16;
pub const SNOW_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:snow_block") as u16;
pub const ICE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:ice") as u16;
/// 新矿种（y 高度带见 `worldgen::ores`）。
pub const COPPER_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:copper_ore") as u16;
pub const GOLD_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:gold_ore") as u16;
pub const REDSTONE_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:redstone_ore") as u16;
pub const DIAMOND_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:diamond_ore") as u16;
pub const EMERALD_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:emerald_ore") as u16;
pub const LAPIS_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:lapis_ore") as u16;
// P45 v2：深层板岩与深板岩矿，同样按 `data/777/blocks.json` 查表（不写死数字）。
pub const DEEPSLATE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate") as u16;
pub const DEEPSLATE_COAL_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate_coal_ore") as u16;
pub const DEEPSLATE_IRON_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate_iron_ore") as u16;
pub const DEEPSLATE_COPPER_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate_copper_ore") as u16;
pub const DEEPSLATE_GOLD_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate_gold_ore") as u16;
pub const DEEPSLATE_REDSTONE_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate_redstone_ore") as u16;
pub const DEEPSLATE_LAPIS_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate_lapis_ore") as u16;
pub const DEEPSLATE_DIAMOND_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate_diamond_ore") as u16;
pub const DEEPSLATE_EMERALD_ORE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:deepslate_emerald_ore") as u16;
/// 草花 / 水下植被（均为非地形、非 MOTION_BLOCKING 装饰方块）。
pub const SHORT_GRASS_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:short_grass") as u16;
pub const TALL_GRASS_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:tall_grass") as u16;
pub const FERN_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:fern") as u16;
pub const DEAD_BUSH_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:dead_bush") as u16;
pub const BUSH_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:bush") as u16;
pub const DANDELION_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:dandelion") as u16;
pub const POPPY_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:poppy") as u16;
pub const CORNFLOWER_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:cornflower") as u16;
pub const SEAGRASS_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:seagrass") as u16;
pub const TALL_SEAGRASS_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:tall_seagrass") as u16;
pub const KELP_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:kelp") as u16;
pub const LILY_PAD_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:lily_pad") as u16;
// ---------------------------------------------------------------------------
// P46 v2：单 chunk 小遗迹与 block light 光源。全部编译期查真实 26.3 ID，
// v1 基底不引用这些常量。
// ---------------------------------------------------------------------------
pub const STONE_BRICKS_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:stone_bricks") as u16;
pub const CHISELED_STONE_BRICKS_BLOCK_ID: u16 =
    registry_id(mc_registry::BLOCKS, "minecraft:chiseled_stone_bricks") as u16;
/// 亮度查表值来自 `data/777/full/blocks.json` 各 state 的 `luminance`。
pub const TORCH_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:torch") as u16;
pub const SOUL_TORCH_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:soul_torch") as u16;
pub const LANTERN_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:lantern") as u16;
pub const SEA_LANTERN_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:sea_lantern") as u16;
pub const GLOWSTONE_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:glowstone") as u16;
pub const LAVA_BLOCK_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:lava") as u16;
/// P27 树叶 decay 判定半径（Chebyshev）：3 格内无橡木原木即失支撑。
/// 原版是树叶 distance 属性逐格传播（7 格），这里简化为到原木的直接距离（见 HANDOFF §5）。
pub const LEAF_DECAY_RADIUS: i32 = 3;
/// P27 树叶 decay 延迟的全局 tick 数（50ms/拍 → 100 拍 = 5s；测试经
/// `set_leaf_decay_ticks` 调小，仿 P22/P26）。
pub const LEAF_DECAY_TICKS: u64 = 100;
/// `Blocks.WHEAT.defaultBlockState()`（age=0）：`data/777/blocks.json` 的
/// `wheat.default_state_id=6867`；`data/777/full/blocks.json` 的 wheat `states`
/// 数组按 age 升序排列 `id: 6867..=6874`，故 age N 的 blockstate = base + N。
/// P27 起可在耕地（`FARMLAND_BLOCK_ID`）上种植并加速生长（`is_crop_soil`）。
/// P33 起小麦是 [`crops::CROPS`] 的一员；常量/函数与旧版逐字节等价，供旧路径使用。
pub const WHEAT_CROP_BASE_ID: u16 = registry_id(mc_registry::BLOCKS, "minecraft:wheat") as u16;
/// 小麦最大 age（7 阶段成熟；states 数组共 8 项）。
pub const WHEAT_MAX_AGE: u8 = 7;
/// 小麦每生长一阶段的全局 tick 数（50ms/拍 → 1200 拍 = 60s；固定时长，原版随机 tick 差异见 HANDOFF）。
pub const WHEAT_TICKS_PER_STAGE: u64 = 1200;
/// P26 树苗长成小橡树的全局 tick 数（50ms/拍 → 7200 拍 = 360s = 6min；
/// 生产值 5~10min 区间内；测试经 `set_sapling_ticks` 调小，仿 P22）。
pub const SAPLING_GROW_TICKS: u64 = 7200;
/// P26 运行时小橡树干高度（固定 4；世界生成树 4~6，这里取下限做缩小版）。
pub const SAPLING_TRUNK_H: i32 = 4;

/// 小麦 age → blockstate ID（age > 7 按 7 钳制）。
/// P33：委托通用作物表（`crops::WHEAT`），旧调用方行为不变。
pub fn wheat_stage_id(age: u8) -> u16 {
    crops::crop_stage_id(&crops::WHEAT, age)
}

/// blockstate ID → 小麦 age（非小麦返回 None）。
/// P33：委托通用作物表（`crops::WHEAT`），旧调用方行为不变。
pub fn wheat_age(id: u16) -> Option<u8> {
    crops::crop_age(&crops::WHEAT, id)
}

/// 可种土壤：草方块/泥土（P22）+ 耕地（P27，`decide_plant` 与树苗生长的土壤检查共用；
/// 树苗种在耕地上是 P27 简化（原版树苗不吃耕地），见 HANDOFF §5）。
pub fn is_crop_soil(id: u16) -> bool {
    id == GRASS_BLOCK_ID || id == DIRT_BLOCK_ID || id == FARMLAND_BLOCK_ID
}

/// P26 运行时小橡树形状：复刻 `worldgen::WorldGen::write_tree` 的冠形逻辑，
/// 干高固定为 [`SAPLING_TRUNK_H`]（世界生成树 4~6 的缩小版，偏差见 HANDOFF）。
/// 输入树苗格 `(sx,sy,sz)`（树干基座 = 树苗本身所在格）；
/// 返回 trunk 在前、leaves 在后的有序列表（调用方先放 trunk 再放 leaves，
/// leaves 被 trunk 占位时跳过，与世界生成“trunk 可覆盖 leaves、leaves 只盖 air”同口径；
/// 运行时在此基础上要求目标为空气才放，见 `DropState::tick_saplings`）。
pub fn sapling_oak_blocks(sx: i32, sy: i32, sz: i32) -> Vec<((i32, i32, i32), u16)> {
    crops::tree_blocks(OAK_LOG_BLOCK_ID, OAK_LEAVES_BLOCK_ID, sx, sy, sz)
}

/// 按 y 分层的地形带：y <= GROUND_LEVEL 石头，61..=63 泥土，64 草方块，其余空气。
pub const GROUND_LEVEL: i32 = 60;
/// 泥土带顶（61..=DIRT_TOP 为泥土）。
pub const DIRT_TOP: i32 = 63;
/// 草方块所在高度（单层盖顶）。
pub const GRASS_LEVEL: i32 = 64;
/// 24 节对应 y=-64..320（min_y=-64，每节 16 格）。
pub const MIN_Y: i32 = -64;
pub const SECTION_COUNT: usize = 24;

/// 按 y 查地形方块（与 x/z 无关，全图同形；P3 按种子接真实高度图）。
/// y<=60 石头，61..=63 泥土，64 草方块，其余空气。
pub fn block_at_y(y: i32) -> u16 {
    if y <= GROUND_LEVEL {
        STONE_BLOCK_ID
    } else if y <= DIRT_TOP {
        DIRT_BLOCK_ID
    } else if y == GRASS_LEVEL {
        GRASS_BLOCK_ID
    } else {
        AIR_BLOCK_ID
    }
}

/// 旧占位：确定性平面 Chunk（save/load 测试兼容）。Play 网络路径请用
/// [`worldgen::WorldGen::generate`]。
/// 按 y 分层填节：整节同值记 Uniform，跨层混合节（48..63 石头+泥土、
/// 64..79 草+空气）记 Paletted 占位，P3 填真实 paletted 容器。
/// `empty_flat` 保留，行为不变。
pub fn chunk_for(pos: mc_core::ChunkPos) -> Chunk {
    let mut sections = Vec::with_capacity(SECTION_COUNT);
    for idx in 0..SECTION_COUNT {
        let base = MIN_Y + idx as i32 * 16;
        let first = block_at_y(base);
        let last = block_at_y(base + 15);
        if first == last {
            sections.push(SectionKind::Uniform(first));
        } else {
            sections.push(SectionKind::Paletted);
        }
    }
    Chunk { pos, sections }
}

/// 视距内区块列表（P5b）：以 `center` 为中心、切比雪夫半径 `radius` 的正方形。
/// `radius=4` 时 `(2*4+1)^2=81` 块。按环递增排序（中心先发），环内按 dx/dz 递增，
/// 保证每 tick 限流 1 块时出生点区块最先到客户端。`radius<0` 返回空。
pub fn view_chunks(center: mc_core::ChunkPos, radius: i32) -> Vec<mc_core::ChunkPos> {
    if radius < 0 {
        return Vec::new();
    }
    let side = (radius as usize) * 2 + 1;
    let mut out = Vec::with_capacity(side * side);
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            out.push(mc_core::ChunkPos(center.0 + dx, center.1 + dz));
        }
    }
    // 中心优先：环（chebyshev 距离）递增，环内 dx/dz 递增（确定性顺序）。
    out.sort_by_key(|p| {
        let dx = p.0 - center.0;
        let dz = p.1 - center.1;
        (dx.abs().max(dz.abs()), dx, dz)
    });
    out
}
/// Chunk 网络字节：24节真布局串接（每节8B起，复用 mc-protocol 单值/间接容器）。
/// 来源：Chunk_format#Chunk_Section结构 + Paletted容器（1.21.5+无data_len），与protocol对齐。
/// 按高度区分：空气节全零占位；均匀石头节调色板记石头ID、block_count=4096；
/// 混合节（石头+泥土、草+空气）走间接palette（BPE=4，4096项真实数据），
/// 保证出生点所在 section 有几何体，客户端才会完成 “player section compiled”。
pub fn chunk_network_bytes(chunk: &Chunk) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.sections.len() * 8);
    let mut states = [0i32; mc_protocol::packets::SECTION_VOLUME];
    for (idx, kind) in chunk.sections.iter().enumerate() {
        match *kind {
            SectionKind::Uniform(AIR_BLOCK_ID) => {
                mc_protocol::packets::encode_empty_section_placeholder(&mut out);
            }
            SectionKind::Uniform(id) => {
                // 均匀非空节：单值palette记该blockstate ID，fluid=0，非空气数4096。
                mc_protocol::packets::encode_uniform_section_single(
                    id as i32, mc_protocol::packets::PLAINS_BIOME_ID_777, 4096, 0, &mut out,
                );
            }
            SectionKind::Paletted | SectionKind::Direct => {
                // 混合节：按 y 层生成真实 4096 项（局部索引 y*256+z*16+x），走间接 palette。
                let base = MIN_Y + idx as i32 * 16;
                for dy in 0..16 {
                    let id = block_at_y(base + dy) as i32;
                    states[dy as usize * 256..(dy as usize + 1) * 256].fill(id);
                }
                mc_protocol::packets::encode_block_section(
                    &states, mc_protocol::packets::PLAINS_BIOME_ID_777, &mut out,
                );
            }
        }
    }
    out
}

/// 真实 MOTION_BLOCKING 高度图：当前平原地形最高非空气方块 = 草方块 y=64，
/// 高度值 = 65 - min_y(-64) = 129。
pub fn chunk_heightmaps_bytes() -> Vec<u8> {
    mc_protocol::packets::motion_blocking_heightmap_uniform((GRASS_LEVEL + 1) - MIN_Y)
}

/// 平原天空光照：草方块所在 section（y=64..79，index 8）及以上 15 级全亮；
/// 地下 section 无光照数组（客户端按 0 处理）。block light 为空。
pub fn chunk_light_bytes() -> Vec<u8> {
    let first_lit = ((GRASS_LEVEL - MIN_Y) / 16) as u32;
    let mask = ((1u64 << SECTION_COUNT) - 1) & !((1u64 << first_lit) - 1);
    let mut out = Vec::new();
    mc_protocol::packets::encode_sky_light_data(mask, 15, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_ids_come_from_registry_table() {
        // 防止 codegen 数据漂移：编译期常量必须仍等于 26.3 实测值，
        // 且与公开查询 API 的运行时结果一致。
        for (name, expected, id) in [
            ("minecraft:air", 0, AIR_BLOCK_ID),
            ("minecraft:stone", 1, STONE_BLOCK_ID),
            ("minecraft:cobblestone", 14, COBBLESTONE_BLOCK_ID),
            ("minecraft:grass_block", 9, GRASS_BLOCK_ID),
            ("minecraft:dirt", 10, DIRT_BLOCK_ID),
            ("minecraft:water", 89, WATER_BLOCK_ID),
            ("minecraft:sand", 121, SAND_BLOCK_ID),
            ("minecraft:oak_log", 140, OAK_LOG_BLOCK_ID),
            ("minecraft:oak_leaves", 294, OAK_LEAVES_BLOCK_ID),
            ("minecraft:oak_sapling", 30, OAK_SAPLING_BLOCK_ID),
            ("minecraft:coal_ore", 136, COAL_ORE_BLOCK_ID),
            ("minecraft:iron_ore", 134, IRON_ORE_BLOCK_ID),
            ("minecraft:wheat", 6867, WHEAT_CROP_BASE_ID),
        ] {
            assert_eq!(id as u32, expected, "wrong blockstate ID for {name}");
            assert_eq!(Some(id as u32), mc_registry::block_id(name));
        }
    }

    #[test]
    fn wheat_stages_are_base_plus_age() {
        // `data/777/full/blocks.json` wheat states 按 age 升序 id 6867..=6874;
        // default_state_id=6867 即 age=0。实现只从 JSON 取 base，偏移是数据顺序本身。
        assert_eq!(WHEAT_CROP_BASE_ID, 6867);
        for age in 0..=WHEAT_MAX_AGE {
            assert_eq!(wheat_stage_id(age), 6867 + age as u16, "age={age}");
            assert_eq!(wheat_age(6867 + age as u16), Some(age));
        }
        assert_eq!(wheat_age(6866), None);
        assert_eq!(wheat_age(6875), None);
        assert_eq!(wheat_age(STONE_BLOCK_ID), None);
        assert_eq!(wheat_stage_id(99), wheat_stage_id(WHEAT_MAX_AGE), "over-age clamps");
    }

    #[test]
    fn p27_farmland_and_decay_consts_come_from_registry() {
        // P27 新增 ID 必须来自 codegen 查表（`data/777/*.json`），不许手写数字。
        assert_eq!(FARMLAND_BLOCK_ID, 6875, "farmland default_state_id (dry)");
        assert_eq!(Some(FARMLAND_BLOCK_ID as u32), mc_registry::block_id("minecraft:farmland"));
        assert_eq!(Some(FARMLAND_BLOCK_ID as u32), mc_registry::block_id("farmland"));
        // 土壤集合：草/泥土（P22）+ 耕地（P27）。
        assert!(is_crop_soil(GRASS_BLOCK_ID));
        assert!(is_crop_soil(DIRT_BLOCK_ID));
        assert!(is_crop_soil(FARMLAND_BLOCK_ID));
        assert!(!is_crop_soil(STONE_BLOCK_ID));
        assert!(!is_crop_soil(AIR_BLOCK_ID));
        assert!(!is_crop_soil(WHEAT_CROP_BASE_ID));
        // decay 参数 sanity：半径取小（3 格，原版 distance 传播 7 格的简化，见 HANDOFF）。
        assert_eq!(LEAF_DECAY_RADIUS, 3);
        assert!(LEAF_DECAY_TICKS >= 1, "decay delay must be a positive tick count");
    }

    /// P44：v2 新方块 ID 必须随 `data/777` codegen 漂移，禁止手写数字。
    #[test]
    fn p44_worldgen_block_ids_come_from_registry() {
        for (name, expected, id) in [
            ("minecraft:gravel", 127, GRAVEL_BLOCK_ID),
            ("minecraft:sandstone", 677, SANDSTONE_BLOCK_ID),
            ("minecraft:coarse_dirt", 11, COARSE_DIRT_BLOCK_ID),
            ("minecraft:podzol", 13, PODZOL_BLOCK_ID),
            ("minecraft:clay", 8616, CLAY_BLOCK_ID),
            ("minecraft:snow_block", 8598, SNOW_BLOCK_ID),
            ("minecraft:ice", 8597, ICE_BLOCK_ID),
            ("minecraft:copper_ore", 31147, COPPER_ORE_BLOCK_ID),
            ("minecraft:gold_ore", 132, GOLD_ORE_BLOCK_ID),
            ("minecraft:redstone_ore", 8552, REDSTONE_ORE_BLOCK_ID),
            ("minecraft:diamond_ore", 6863, DIAMOND_ORE_BLOCK_ID),
            ("minecraft:emerald_ore", 11315, EMERALD_ORE_BLOCK_ID),
            ("minecraft:lapis_ore", 662, LAPIS_ORE_BLOCK_ID),
            ("minecraft:short_grass", 2363, SHORT_GRASS_BLOCK_ID),
            ("minecraft:tall_grass", 14771, TALL_GRASS_BLOCK_ID),
            ("minecraft:fern", 2364, FERN_BLOCK_ID),
            ("minecraft:dead_bush", 2365, DEAD_BUSH_BLOCK_ID),
            ("minecraft:bush", 2366, BUSH_BLOCK_ID),
            ("minecraft:dandelion", 3813, DANDELION_BLOCK_ID),
            ("minecraft:poppy", 3816, POPPY_BLOCK_ID),
            ("minecraft:cornflower", 3825, CORNFLOWER_BLOCK_ID),
            ("minecraft:seagrass", 2370, SEAGRASS_BLOCK_ID),
            ("minecraft:tall_seagrass", 2372, TALL_SEAGRASS_BLOCK_ID),
            ("minecraft:kelp", 18419, KELP_BLOCK_ID),
            ("minecraft:lily_pad", 10654, LILY_PAD_BLOCK_ID),
        ] {
            assert_eq!(id as u32, expected, "wrong blockstate ID for {name}");
            assert_eq!(Some(id as u32), mc_registry::block_id(name), "registry drift for {name}");
        }
    }

    #[test]
    fn chunk_layer_correct() {
        // 按 y 分层：y<=60 石头，61-63 泥土，64 草，其余空气
        for y in [MIN_Y, -65, -1, 0, 59, 60] {
            assert_eq!(block_at_y(y), STONE_BLOCK_ID, "y={y} should be stone");
        }
        for y in [61, 62, 63] {
            assert_eq!(block_at_y(y), DIRT_BLOCK_ID, "y={y} should be dirt");
        }
        assert_eq!(block_at_y(64), GRASS_BLOCK_ID);
        for y in [65, 79, 80, 319, 320] {
            assert_eq!(block_at_y(y), AIR_BLOCK_ID, "y={y} should be air");
        }
        // section 落位：0..6 整节石头，7/8 混合，9..23 整节空气
        let c = chunk_for(mc_core::ChunkPos(0, 0));
        assert_eq!(c.sections.len(), SECTION_COUNT);
        for (i, s) in c.sections.iter().enumerate().take(7) {
            assert_eq!(*s, SectionKind::Uniform(STONE_BLOCK_ID), "section {i} stone");
        }
        assert_eq!(c.sections[7], SectionKind::Paletted, "48..63 stone+dirt");
        assert_eq!(c.sections[8], SectionKind::Paletted, "64..79 grass+air");
        for (i, s) in c.sections.iter().enumerate().skip(9) {
            assert_eq!(*s, SectionKind::Uniform(AIR_BLOCK_ID), "section {i} air");
        }
        // 确定性：同 pos 同形
        let again = chunk_for(mc_core::ChunkPos(0, 0));
        assert_eq!(c.sections, again.sections);
    }

    #[test]
    fn chunk_for_is_deterministic() {
        let pos = mc_core::ChunkPos(0, 0);
        let a = chunk_for(pos);
        let b = chunk_for(pos);
        assert_eq!(a.sections, b.sections);
        assert_eq!(a.sections.len(), SECTION_COUNT);
        // 不同坐标同样确定（当前全图同形，P3 按种子起伏）
        let c = chunk_for(mc_core::ChunkPos(5, -7));
        assert_eq!(c.sections, a.sections);
        // empty_flat 保留：全空气 24 节
        let flat = Chunk::empty_flat(pos);
        assert!(flat.sections.iter().all(|s| *s == SectionKind::Uniform(0)));
    }

    #[test]
    fn chunk_network_bytes_not_empty() {
        let c = chunk_for(mc_core::ChunkPos(0, 0));
        let bytes = chunk_network_bytes(&c);
        // 7 个均匀石头节(8B) + 2 个间接 palette 混合节(2058B) + 15 个空气节(8B)
        assert_eq!(bytes.len(), 7 * 8 + 2 * 2058 + 15 * 8);
        // 均匀石头节：block_count 4096, fluid 0, 单值 stone=1, biome plains=41
        let stone0 = &bytes[0..8];
        assert_eq!(stone0, &[0x10, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x29]);
        for i in 1..7 {
            assert_eq!(&bytes[i * 8..i * 8 + 8], stone0, "stone section {i}");
        }
        // 混合节7（48..63 石头+泥土）：block_count 4096, fluid 0, BPE=4, palette=[1,10],
        // 之后是 4096 项 ×4bit = 256 longs（无长度前缀）。
        let s7 = &bytes[7 * 8..7 * 8 + 2058];
        assert_eq!(&s7[..4], &[0x10, 0x00, 0x00, 0x00]);
        assert_eq!(s7[4], 4, "BPE=4");
        assert_eq!(s7[5], 2, "palette len=2");
        assert_eq!(&s7[6..8], &[1, 10], "palette=[stone,dirt]");
        // 混合节8（64 草方块 + 65..79 空气）：block_count 256，palette=[9,0]。
        let s8 = &bytes[7 * 8 + 2058..7 * 8 + 2058 + 2058];
        assert_eq!(&s8[..4], &[0x01, 0x00, 0x00, 0x00]);
        assert_eq!(s8[4], 4);
        assert_eq!(s8[5], 2);
        assert_eq!(&s8[6..8], &[9, 0], "palette=[grass_block,air]");
        // 空气节（idx>=9）写 plains=41 的 biome palette，其余字节为0
        let air = &bytes[7 * 8 + 2 * 2058..];
        assert!(air.chunks(8).all(|s| s == [0, 0, 0, 0, 0, 0, 0, 41]), "air sections keep plains biome id");
    }

    #[test]
    fn chunk_heightmaps_is_real_motion_blocking() {
        // 平原地表：草方块 y=64 → height = 65 - (-64) = 129
        let h = chunk_heightmaps_bytes();
        assert_eq!(&h[..3], &[0x01, 0x04, 0x25]);
        assert_eq!(h.len(), 3 + 37 * 8);
        let mut word_bytes = [0u8; 8];
        word_bytes.copy_from_slice(&h[3..11]);
        let first = u64::from_be_bytes(word_bytes);
        for k in 0..7 {
            assert_eq!((first >> (k * 9)) & 0x1ff, 129, "first long item {k}");
        }
        // 最后一个 long 装 256-36*7=4 项，其余为空。
        word_bytes.copy_from_slice(&h[3 + 36 * 8..]);
        let last = u64::from_be_bytes(word_bytes);
        for k in 0..4 {
            assert_eq!((last >> (k * 9)) & 0x1ff, 129, "last long item {k}");
        }
        assert_eq!(last >> (4 * 9), 0, "last long padding must be zero");
    }

    #[test]
    fn chunk_light_is_sky_lit_above_ground() {
        let l = chunk_light_bytes();
        assert_eq!(l.len(), 1 + 3 + 3 + 1 + 16 * (2 + 2048) + 1);
        // sky mask bits 8..23（y=64 及以上 16 个 section），BitSet 字节 [0,0xFF,0xFF]
        assert_eq!(&l[..4], &[3, 0, 0xFF, 0xFF]);
        assert_eq!(l[7], 16);
        assert_eq!(&l[8..10], &[0x80, 0x10]);
        let mut off = 8;
        for _ in 0..16 {
            assert_eq!(&l[off..off + 2], &[0x80, 0x10], "array length 2048");
            off += 2;
            assert!(l[off..off + 2048].iter().all(|&b| b == 0xFF), "sky level 15");
            off += 2048;
        }
        assert_eq!(off, l.len() - 1, "only block array count remains");
    }

    #[test]
    fn view_chunks_radius4_is_81_center_first() {
        let center = mc_core::ChunkPos(0, 0);
        let v = view_chunks(center, 4);
        assert_eq!(v.len(), 81, "radius=4 -> (2*4+1)^2=81");
        // 中心先发：限流 1/tick 时出生点区块最先到
        assert_eq!(v[0], center);
        // 全覆盖：dx/dz in -4..=4 且无重复
        let mut sorted = v.clone();
        sorted.sort_by_key(|p| (p.0, p.1));
        sorted.dedup();
        assert_eq!(sorted.len(), 81);
        for p in &v {
            assert!((p.0 - center.0).abs() <= 4 && (p.1 - center.1).abs() <= 4);
        }
        // 非中心平移：相对偏移不变
        let c2 = mc_core::ChunkPos(-3, 7);
        let v2 = view_chunks(c2, 4);
        assert_eq!(v2.len(), 81);
        assert_eq!(v2[0], c2);
    }

    #[test]
    fn view_chunks_edge_radii() {
        assert_eq!(view_chunks(mc_core::ChunkPos(1, 2), 0), vec![mc_core::ChunkPos(1, 2)]);
        assert!(view_chunks(mc_core::ChunkPos(0, 0), -1).is_empty());
        assert_eq!(view_chunks(mc_core::ChunkPos(0, 0), 1).len(), 9);
    }

    #[test]
    fn sapling_oak_shape_is_deterministic_worldgen_subset() {
        // 确定性：同位点两次形状逐项一致；不同位点只是平移。
        let a = sapling_oak_blocks(0, 70, 0);
        let b = sapling_oak_blocks(0, 70, 0);
        assert_eq!(a, b);
        let c = sapling_oak_blocks(10, 70, -5);
        assert_eq!(a.len(), c.len());
        for (i, (((x, y, z), _), ((cx, cy, cz), _))) in a.iter().zip(c.iter()).enumerate() {
            assert_eq!((cx - x, cy - y, cz - z), (10, 0, -5), "shape must translate rigidly {i}");
        }
        // trunk 4（sy..sy+3）+ leaves 53 = 57 格；trunk 在前。
        assert_eq!(a.len(), 57, "4 trunk + 53 leaves, see HANDOFF P26");
        let trunks: Vec<_> = a.iter().filter(|(_, id)| *id == OAK_LOG_BLOCK_ID).collect();
        assert_eq!(trunks.len(), SAPLING_TRUNK_H as usize);
        for (i, ((x, y, z), _)) in trunks.iter().enumerate() {
            assert_eq!((*x, *z), (0, 0));
            assert_eq!(*y, 70 + i as i32);
        }
        // 冠形贴 worldgen：top-2/top-1 去角 5x5（各 21），top 3x3（9），top+1 十字（5），
        // 减去与树干柱重叠的 3 格 = 53。
        let leaves: Vec<_> = a.iter().filter(|(_, id)| *id == OAK_LEAVES_BLOCK_ID).collect();
        assert_eq!(leaves.len(), 53);
        // 去角层不含四角。
        for (dx, dz) in [(2, 2), (2, -2), (-2, 2), (-2, -2)] {
            assert!(
                !leaves.iter().any(|((x, y, z), _)| (*x, *y, *z) == (dx, 71, dz)),
                "top-2 must drop corners"
            );
            assert!(
                !leaves.iter().any(|((x, y, z), _)| (*x, *y, *z) == (dx, 72, dz)),
                "top-1 must drop corners"
            );
        }
        // 顶十字层只含 |dx|+|dz|<=1。
        for ((x, y, z), _) in leaves {
            if *y == 74 {
                assert!(x.abs() + z.abs() <= 1, "top+1 must be a cross");
            }
        }
    }
}
