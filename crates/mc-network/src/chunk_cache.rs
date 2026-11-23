//! chunk_cache: 已编码 ChunkData(0x2E) 包体缓存，避免跨视距移动/重连时重复生成。
//!
//! 只缓存最终包体 `Vec<u8>`（P16 约 50KB/块），不缓存 `GeneratedChunk`
//! （24×4096×u16 ≈ 196KB/块）。缓存内容由构造时的 seed 对应的 `WorldGen` 生成，
//! 因此一个 `ChunkCache` 只能服务同一 seed；`enter_play` 会在 seed 不匹配时忽略它。
//! 淘汰策略为 FIFO（容量默认 1024，可覆盖 radius=10 的 441 块并留出移动余量）。
//! P16 起 `bodies()` 用 rayon 并行生成/编码一批缺失区块（Join 441 块的主路径）。
use mc_core::ChunkPos;
use mc_protocol::packets;
use mc_world::WorldGen;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// 默认容量：1024 块，足以覆盖 radius=10 视距（441）+ 跨区块移动余量。
pub const DEFAULT_CHUNK_CACHE_CAPACITY: usize = 1024;

/// 编码一个 chunk 的 ChunkData 包体（不含 packet id / 外层长度）。
pub fn encode_chunk_data_body(world: &WorldGen, pos: ChunkPos) -> Vec<u8> {
    let chunk = world.generate(pos);
    let chunk_data = packets::PlayChunkData {
        chunk_x: pos.0,
        chunk_z: pos.1,
        heightmaps_nbt: chunk.heightmaps_bytes(),
        section_bytes: chunk.network_bytes(),
        light_bytes: chunk.light_bytes(),
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
#[derive(Debug)]
pub struct ChunkCache {
    seed: i64,
    capacity: usize,
    inner: Mutex<CacheInner>,
}

impl ChunkCache {
    pub fn new(seed: i64, capacity: usize) -> Self {
        Self {
            seed,
            capacity,
            inner: Mutex::new(CacheInner {
                map: HashMap::new(),
                order: VecDeque::new(),
                stats: ChunkCacheStats::default(),
            }),
        }
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    pub fn capacity(&self) -> usize {
        self.capacity
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
    /// 生成在锁外完成，避免长时间阻塞其他连接。
    pub fn body(&self, pos: ChunkPos) -> Arc<Vec<u8>> {
        {
            let mut inner = self.lock();
            if let Some(hit) = inner.map.get(&pos).cloned() {
                inner.stats.hits += 1;
                return hit;
            }
        }
        let body = Arc::new(encode_chunk_data_body(&WorldGen::new(self.seed), pos));
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
}
