//! chunk_cache: 已编码 ChunkData(0x2E) 包体缓存，避免跨视距移动/重连时重复生成。
//!
//! 只缓存最终包体 `Vec<u8>`（P16 约 50KB/块），不缓存 `GeneratedChunk`
//! （24×4096×u16 ≈ 196KB/块）。缓存内容由构造时的 seed 对应的 `WorldGen` 生成，
//! 因此一个 `ChunkCache` 只能服务同一 seed；`enter_play` 会在 seed 不匹配时忽略它。
//! 淘汰策略为 FIFO（容量默认 1024，可覆盖 radius=10 的 441 块并留出移动余量）。
//! P16 起 `bodies()` 用 rayon 并行生成/编码一批缺失区块（Join 441 块的主路径）。
use mc_core::ChunkPos;
use mc_protocol::packets;
use mc_world::{GeneratorVersion, MutableWorld, WorldGen};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tracing::warn;

/// 默认容量：1024 块，足以覆盖 radius=10 视距（441）+ 跨区块移动余量。
pub const DEFAULT_CHUNK_CACHE_CAPACITY: usize = 1024;

/// 编码一个 chunk 的 ChunkData 包体（不含 packet id / 外层长度）。
/// 纯基底路径（无 override 时用；与 [`encode_chunk_data_body_mutable`] 同编码）.
pub fn encode_chunk_data_body(world: &WorldGen, pos: ChunkPos) -> Vec<u8> {
    encode_generated(&world.generate(pos), world)
}

/// 编码一个带 override 的 chunk（P17 挖掘/放置后重发用）。
pub fn encode_chunk_data_body_mutable(world: &MutableWorld, pos: ChunkPos) -> Vec<u8> {
    let gen = WorldGen::with_version(world.seed(), world.generator_version());
    encode_generated(&world.generate_chunk(pos), &gen)
}

/// v1 走冻结的单值 biome / 简化天空光；v2 走多 biome 容器 + 逐列天空光。
/// 两者都是同一个 0x2E 包体结构，只是 Data/Light 内容不同。
fn encode_generated(chunk: &mc_world::GeneratedChunk, world: &WorldGen) -> Vec<u8> {
    let (section_bytes, light_bytes) = match world.version {
        GeneratorVersion::V1 => (chunk.network_bytes(), chunk.light_bytes()),
        GeneratorVersion::V2 => (chunk.network_bytes_v2(world), chunk.light_bytes_v2()),
    };
    let chunk_data = packets::PlayChunkData {
        chunk_x: chunk.pos.0,
        chunk_z: chunk.pos.1,
        heightmaps_nbt: chunk.heightmaps_bytes(),
        section_bytes,
        light_bytes,
    };
    let mut body = Vec::new();
    chunk_data.encode_body(&mut body);
    body
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChunkCacheStats {
    pub hits: u64,
    pub misses: u64,
}

#[derive(Debug)]
struct CacheInner {
    map: HashMap<ChunkPos, Arc<Vec<u8>>>,
    /// FIFO：命中不移动，只有新插入才 push_back；淘汰从队首跳过已不存在的项。
    order: VecDeque<ChunkPos>,
    stats: ChunkCacheStats,
    /// 缓存包体绑定的生成器版本；变更时必须全量失效（基底变了）。
    version: GeneratorVersion,
}

impl Default for CacheInner {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            stats: ChunkCacheStats::default(),
            version: GeneratorVersion::V1,
        }
    }
}

impl CacheInner {
    fn evict_to_capacity(&mut self, capacity: usize) {
        while self.map.len() > capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.map.remove(&oldest);
        }
    }
}

/// 线程安全（`Mutex`）的 ChunkData 包体缓存，可被多连接共享（`Arc<ChunkCache>`）。
///
/// P17：可选绑定 [`MutableWorld`]；绑定后编码走 `generate_chunk`（基底 + override），
/// 方块编辑后调 [`ChunkCache::invalidate`] 丢弃对应 chunk，下次发送即含新方块。
#[derive(Debug)]
pub struct ChunkCache {
    seed: i64,
    capacity: usize,
    mutable: Option<Arc<MutableWorld>>,
    inner: Mutex<CacheInner>,
}

impl ChunkCache {
    /// 默认固定 v1（legacy）：旧调用方/旧测试行为完全不变。
    pub fn new(seed: i64, capacity: usize) -> Self {
        Self::new_with_version(seed, capacity, GeneratorVersion::V1)
    }

    /// 显式版本：缓存内容从这一刻起由 `(seed, version)` 决定。
    pub fn new_with_version(
        seed: i64,
        capacity: usize,
        version: GeneratorVersion,
    ) -> Self {
        let mut inner = CacheInner::default();
        inner.version = version;
        Self { seed, capacity, mutable: None, inner: Mutex::new(inner) }
    }

    /// 绑定可变世界（P17 挖掘/放置后重编用）；seed 不一致时 warn（仍绑定，
    /// 但编码用 MutableWorld 自己的 seed，调用方应保证两者一致）。
    pub fn with_mutable(mut self, mutable: Arc<MutableWorld>) -> Self {
        if mutable.seed() != self.seed {
            warn!(
                "chunk cache seed {} != mutable world seed {}, encoding follows the mutable world",
                self.seed,
                mutable.seed()
            );
        }
        if self.version() != mutable.generator_version() {
            warn!(
                "chunk cache generator {:?} != mutable world {:?}, encoding follows the mutable world",
                self.version(),
                mutable.generator_version()
            );
        }
        self.mutable = Some(mutable);
        self
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 当前缓存绑定的生成器版本。
    pub fn version(&self) -> GeneratorVersion {
        self.lock().version
    }

    /// 切换版本：只要版本变化就丢弃**全部**已编码包体（基底算法变了，
    /// 任何旧条目都不能复用）。返回是否真的发生了切换。
    pub fn set_version(&self, version: GeneratorVersion) -> bool {
        let mut inner = self.lock();
        if inner.version == version {
            return false;
        }
        inner.version = version;
        inner.map.clear();
        inner.order.clear();
        true
    }

    /// 清空全部缓存（供存档版本校验失败/配置重载时用）。
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.map.clear();
        inner.order.clear();
    }

    /// 当前缓存块数。
    pub fn len(&self) -> usize {
        self.lock().map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn stats(&self) -> ChunkCacheStats {
        self.lock().stats
    }

    /// 是否已缓存该区块（测试/诊断用）。
    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.lock().map.contains_key(&pos)
    }

    /// 取包体：命中直接返回同一 `Arc`；未命中则用本 cache 的 seed 生成并插入。
    /// 绑定 MutableWorld 后走基底 + override（含玩家编辑）。
    /// 生成在锁外完成，避免长时间阻塞其他连接。
    pub fn body(&self, pos: ChunkPos) -> Arc<Vec<u8>> {
        {
            let mut inner = self.lock();
            if let Some(hit) = inner.map.get(&pos).cloned() {
                inner.stats.hits += 1;
                return hit;
            }
        }
        let body = match &self.mutable {
            Some(m) => Arc::new(encode_chunk_data_body_mutable(m, pos)),
            None => {
                let version = self.version();
                Arc::new(encode_chunk_data_body(
                    &WorldGen::with_version(self.seed, version),
                    pos,
                ))
            }
        };
        let mut inner = self.lock();
        // 生成期间可能有其他线程先插入同一区块：复用已有字节，保持确定性。
        if let Some(existing) = inner.map.get(&pos).cloned() {
            inner.stats.hits += 1;
            return existing;
        }
        inner.stats.misses += 1;
        if self.capacity == 0 {
            return body;
        }
        inner.order.push_back(pos);
        inner.map.insert(pos, Arc::clone(&body));
        inner.evict_to_capacity(self.capacity);
        body
    }

    /// P17：方块编辑后使该 chunk 缓存失效（返回是否命中旧条目）。
    /// 下次发送时按当前 override 重编；FIFO 队列里的陈旧位置由淘汰时顺带清理。
    pub fn invalidate(&self, pos: ChunkPos) -> bool {
        self.lock().map.remove(&pos).is_some()
    }

    /// 批量取包体（Join/跨区块一批）：顺序与 `positions` 一致，未命中项用 rayon
    /// 并行生成+编码，缩短 441 块初始批的墙钟时间；命中项仍直接复用 `Arc`。
    /// 只做纯 CPU 生成，不持有锁跨线程，行为与逐个 [`ChunkCache::body`] 相同。
    pub fn bodies(&self, positions: &[ChunkPos]) -> Vec<Arc<Vec<u8>>> {
        use rayon::prelude::*;
        positions.par_iter().map(|&pos| self.body(pos)).collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CacheInner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_world::DEFAULT_SEED;

    fn position(x: i32, z: i32) -> ChunkPos {
        ChunkPos(x, z)
    }

    #[test]
    fn hit_returns_same_bytes_as_direct_encode() {
        let pos = position(23, 23);
        let cache = ChunkCache::new(DEFAULT_SEED, 8);
        let first = cache.body(pos);
        let second = cache.body(pos);
        let direct = encode_chunk_data_body(&WorldGen::new(DEFAULT_SEED), pos);
        assert!(Arc::ptr_eq(&first, &second), "cache hit must reuse the Arc");
        assert_eq!(first.as_slice(), direct.as_slice(), "cached bytes must match direct encoding");
        let stats = cache.stats();
        assert_eq!((stats.hits, stats.misses), (1, 1), "second lookup must be a hit");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn fifo_evicts_oldest_beyond_capacity() {
        let cache = ChunkCache::new(DEFAULT_SEED, 2);
        let a = position(0, 0);
        let b = position(1, 0);
        let c = position(2, 0);
        let _ = cache.body(a);
        let _ = cache.body(b);
        assert_eq!(cache.len(), 2);
        let _ = cache.body(c);
        assert_eq!(cache.len(), 2, "capacity must be respected");
        assert!(!cache.contains(a), "oldest chunk evicted first");
        assert!(cache.contains(b));
        assert!(cache.contains(c));
        // 被淘汰的 a 再取是 miss，且字节仍与直接编码一致。
        let again = cache.body(a);
        assert_eq!(again.as_slice(), encode_chunk_data_body(&WorldGen::new(DEFAULT_SEED), a).as_slice());
        assert_eq!(cache.stats().misses, 4);
    }

    #[test]
    fn different_chunks_do_not_share_data() {
        let cache = ChunkCache::new(DEFAULT_SEED, 8);
        let a = cache.body(position(0, 0));
        let b = cache.body(position(1, 0));
        assert_ne!(a.as_slice(), b.as_slice(), "different chunks must not share bytes");
        let (chunk_a, _) = packets::PlayChunkData::decode_body(&a).unwrap();
        let (chunk_b, _) = packets::PlayChunkData::decode_body(&b).unwrap();
        assert_eq!((chunk_a.chunk_x, chunk_a.chunk_z), (0, 0));
        assert_eq!((chunk_b.chunk_x, chunk_b.chunk_z), (1, 0));
    }

    #[test]
    fn bodies_preserve_order_and_match_individual_body() {
        let cache = ChunkCache::new(DEFAULT_SEED, 8);
        let positions = [position(23, 23), position(24, 23), position(22, 24)];
        let bodies = cache.bodies(&positions);
        assert_eq!(bodies.len(), positions.len());
        for (body, &pos) in bodies.iter().zip(positions.iter()) {
            assert_eq!(
                body.as_slice(),
                encode_chunk_data_body(&WorldGen::new(DEFAULT_SEED), pos).as_slice(),
                "parallel bodies must match sequential encoding for {pos:?}"
            );
            assert!(cache.contains(pos), "parallel batch must populate the cache");
        }
        assert_eq!(cache.len(), positions.len());
        // 第二次批量：全部命中，返回同一 Arc。
        let again = cache.bodies(&positions);
        for (a, b) in again.iter().zip(bodies.iter()) {
            assert!(Arc::ptr_eq(a, b));
        }
        assert_eq!(cache.stats().hits, positions.len() as u64);
    }

    #[test]
    fn zero_capacity_still_generates_without_storing() {
        let cache = ChunkCache::new(DEFAULT_SEED, 0);
        let body = cache.body(position(0, 0));
        assert!(!body.is_empty());
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.capacity(), 0);
    }

    #[test]
    fn invalidate_drops_chunk_and_rebuilds_with_override() {
        use mc_world::MutableWorld;
        let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
        let cache = ChunkCache::new(DEFAULT_SEED, 8).with_mutable(Arc::clone(&mutable));
        let pos = position(23, 23);
        let before = cache.body(pos);
        assert!(cache.contains(pos));
        // 未编辑时与纯基底逐字节一致（override 为空无污染）。
        assert_eq!(
            before.as_slice(),
            encode_chunk_data_body(&WorldGen::new(DEFAULT_SEED), pos).as_slice()
        );
        // 在出生 chunk 挖一格：失效后重编必须含空气。
        let s = WorldGen::new(DEFAULT_SEED).spawn_pos();
        mutable.set(s.0, s.1 - 1, s.2, mc_world::AIR_BLOCK_ID);
        assert!(cache.invalidate(pos), "edited chunk must have a cached entry");
        assert!(!cache.contains(pos));
        let after = cache.body(pos);
        assert_ne!(before.as_slice(), after.as_slice(), "rebuilt body must contain the edit");
        let (chunk, _) = packets::PlayChunkData::decode_body(&after).unwrap();
        assert_eq!((chunk.chunk_x, chunk.chunk_z), (23, 23));
        // 未命中失效返回 false，不 panic。
        assert!(!cache.invalidate(position(99, -99)));
    }

    #[test]
    fn cache_seed_is_isolated_per_instance() {
        let a = ChunkCache::new(DEFAULT_SEED, 4);
        let b = ChunkCache::new(987654321, 4);
        assert_eq!(a.seed(), DEFAULT_SEED);
        assert_eq!(b.seed(), 987654321);
        assert_ne!(
            a.body(position(23, 23)).as_slice(),
            b.body(position(23, 23)).as_slice(),
            "different seeds must produce different chunk bodies"
        );
    }

    /// P44：生成器版本变化必须使全部已缓存包体失效，且新包体按 v2 基底生成。
    #[test]
    fn generator_version_change_invalidates_whole_cache() {
        let pos = position(23, 23);
        let cache = ChunkCache::new(DEFAULT_SEED, 8);
        assert_eq!(cache.version(), GeneratorVersion::V1);
        let before = cache.body(pos);
        assert!(cache.contains(pos));
        assert!(cache.set_version(GeneratorVersion::V2));
        assert_eq!(cache.version(), GeneratorVersion::V2);
        assert_eq!(cache.len(), 0, "version switch must drop every cached body");
        assert!(!cache.contains(pos));
        let after = cache.body(pos);
        assert_ne!(before.as_slice(), after.as_slice(), "v2 body must differ from v1");
        assert_eq!(
            after.as_slice(),
            encode_chunk_data_body(
                &WorldGen::with_version(DEFAULT_SEED, GeneratorVersion::V2),
                pos
            )
            .as_slice()
        );
        // 相同版本重复设置不失效。
        assert!(!cache.set_version(GeneratorVersion::V2));
        assert!(cache.contains(pos));
    }

    /// P46：v2 包体的 LightData 合并了 block light；v1 仍不发 block 数组。
    #[test]
    fn v2_chunk_body_carries_block_light_while_v1_light_stays_empty() {
        use mc_protocol::read_varint;
        // 前两个 BitSet 分别是 sky mask / block mask（VarInt 字节数 + 小端 bit 字节）。
        let masks_of = |light: &[u8]| -> (u64, u64) {
            let mut off = 0;
            let mut out = [0u64; 2];
            for slot in out.iter_mut().take(2) {
                let (nbytes, n) = read_varint(&light[off..]).unwrap();
                off += n;
                for (bi, b) in light[off..off + nbytes as usize].iter().enumerate() {
                    *slot |= (*b as u64) << (bi * 8);
                }
                off += nbytes as usize;
            }
            (out[0], out[1])
        };
        let gen = WorldGen::with_version(42, GeneratorVersion::V2);
        let mut found = false;
        for cx in -8..8 {
            for cz in -8..8 {
                let pos = position(cx, cz);
                let chunk = gen.generate(pos);
                if !chunk.sections.iter().any(|s| s.iter().any(|&id| id == mc_world::LANTERN_BLOCK_ID)) {
                    continue;
                }
                found = true;
                let v2 = encode_chunk_data_body(&gen, pos);
                let (packet, _) = packets::PlayChunkData::decode_body(&v2).unwrap();
                let (sky, block) = masks_of(&packet.light_bytes);
                assert_ne!(block, 0, "ruin lantern must emit block light");
                assert_ne!(sky, 0, "sky light still present");
                let v1 = encode_chunk_data_body(&WorldGen::new(42), pos);
                let (v1_packet, _) = packets::PlayChunkData::decode_body(&v1).unwrap();
                let (_, v1_block) = masks_of(&v1_packet.light_bytes);
                assert_eq!(v1_block, 0, "v1 block light must stay empty");
                break;
            }
            if found { break; }
        }
        assert!(found, "seed 42 should have at least one ruin in the sampled chunks");
    }

    #[test]
    fn new_with_version_generates_v2_bodies_and_stays_deterministic() {
        let cache = ChunkCache::new_with_version(42, 8, GeneratorVersion::V2);
        let pos = position(4, 7);
        let a = cache.body(pos);
        let b = cache.body(pos);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(
            a.as_slice(),
            encode_chunk_data_body(&WorldGen::with_version(42, GeneratorVersion::V2), pos).as_slice()
        );
        assert_ne!(
            a.as_slice(),
            encode_chunk_data_body(&WorldGen::with_version(42, GeneratorVersion::V1), pos).as_slice()
        );
    }
}
