//! chunk_view: 玩家视距区块集合（已发送/待卸载）追踪，纯逻辑无 IO。
//! 26.3 客户端要求中心 section 的 4 正 + 4 斜邻居都存在才编译；初始与跨区块
//! 都按 radius=4（81 块）维护。`plan` 只在中心变化时调用，返回本批要发的区块
//! （中心优先顺序，直接用于 ChunkBatchStart/Finished）和要发的 UnloadChunk。
use mc_core::ChunkPos;
use std::collections::HashSet;

/// Play 视距半径（切比雪夫）：与 mc-server `VIEW_RADIUS` / 初始 81 块一致。
pub const PLAY_VIEW_RADIUS: i32 = 4;
/// `(2*4+1)^2`。
pub const PLAY_VIEW_CHUNK_COUNT: usize = 81;

/// 中心变化后的一批区块变更。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkViewUpdate {
    /// 需要发送 ChunkData 的区块，`mc_world::view_chunks` 的中心优先顺序。
    pub add: Vec<ChunkPos>,
    /// 需要发送 UnloadChunk 的区块，按 (x,z) 排序保证确定性。
    pub remove: Vec<ChunkPos>,
}

impl ChunkViewUpdate {
    pub fn is_empty(&self) -> bool {
        self.add.is_empty() && self.remove.is_empty()
    }
}

/// 单个玩家的视距区块集合。
#[derive(Debug, Clone)]
pub struct ChunkView {
    center: ChunkPos,
    sent: HashSet<ChunkPos>,
}

impl ChunkView {
    pub fn new(center: ChunkPos) -> Self {
        Self { center, sent: HashSet::new() }
    }

    pub fn center(&self) -> ChunkPos {
        self.center
    }

    pub fn sent_count(&self) -> usize {
        self.sent.len()
    }

    pub fn sent_contains(&self, pos: ChunkPos) -> bool {
        self.sent.contains(&pos)
    }

    /// 以 `new_center` 重新规划视距：更新内部已发送集合，并返回本次要发/要卸的区块。
    /// 初始调用（sent 为空）返回全部 81 块；同一中心重复调用返回空变更。
    pub fn plan(&mut self, new_center: ChunkPos) -> ChunkViewUpdate {
        self.center = new_center;
        let desired_list = mc_world::view_chunks(new_center, PLAY_VIEW_RADIUS);
        let desired: HashSet<ChunkPos> = desired_list.iter().copied().collect();
        let add: Vec<ChunkPos> = desired_list
            .into_iter()
            .filter(|p| !self.sent.contains(p))
            .collect();
        let mut remove: Vec<ChunkPos> = self
            .sent
            .iter()
            .copied()
            .filter(|p| !desired.contains(p))
            .collect();
        remove.sort_by_key(|p| (p.0, p.1));
        self.sent.retain(|p| desired.contains(p));
        self.sent.extend(add.iter().copied());
        ChunkViewUpdate { add, remove }
    }

    /// 按 26.3 语义把方块坐标转区块坐标（负坐标用 div_euclid）；非法/越界返回 None。
    pub fn chunk_of_block(x: f64, z: f64) -> Option<ChunkPos> {
        if !x.is_finite() || !z.is_finite() {
            return None;
        }
        let bx = x.floor().clamp(-30_000_000.0, 30_000_000.0) as i32;
        let bz = z.floor().clamp(-30_000_000.0, 30_000_000.0) as i32;
        Some(ChunkPos(bx.div_euclid(16), bz.div_euclid(16)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_plan_is_81_center_first() {
        let center = ChunkPos(23, 23);
        let mut view = ChunkView::new(center);
        let u = view.plan(center);
        assert_eq!(u.add.len(), PLAY_VIEW_CHUNK_COUNT);
        assert_eq!(u.add[0], center, "spawn chunk first");
        assert!(u.remove.is_empty());
        assert_eq!(view.sent_count(), 81);
        let mut sorted = u.add.clone();
        sorted.sort_by_key(|p| (p.0, p.1));
        sorted.dedup();
        assert_eq!(sorted.len(), 81);
    }

    #[test]
    fn crossing_one_chunk_adds_nine_removes_nine_without_duplicates() {
        let mut view = ChunkView::new(ChunkPos(23, 23));
        view.plan(ChunkPos(23, 23));
        let u = view.plan(ChunkPos(24, 23));
        assert_eq!(u.add.len(), 9);
        assert_eq!(u.remove.len(), 9);
        assert!(u.add.iter().all(|p| p.0 == 28 && (19..=27).contains(&p.1)));
        assert!(u.remove.iter().all(|p| p.0 == 19 && (19..=27).contains(&p.1)));
        assert_eq!(view.sent_count(), 81);
        // 已发送集合里不应有重复语义：add 与 remove 不相交，add 全在 sent 中。
        for p in &u.add {
            assert!(view.sent_contains(*p));
            assert!(!u.remove.contains(p));
        }
        for p in &u.remove {
            assert!(!view.sent_contains(*p));
        }
        // 同一中心再 plan：没有变更、不重复发。
        assert!(view.plan(ChunkPos(24, 23)).is_empty());
    }

    #[test]
    fn zigzag_reuses_previous_chunks() {
        let mut view = ChunkView::new(ChunkPos(0, 0));
        view.plan(ChunkPos(0, 0));
        let east = view.plan(ChunkPos(1, 0));
        assert!(east.add.iter().all(|p| p.0 == 5));
        let back = view.plan(ChunkPos(0, 0));
        assert!(back.add.iter().all(|p| p.0 == -4));
        assert_eq!(back.add.len(), 9);
        assert!(back.remove.iter().all(|p| p.0 == 5));
        assert_eq!(view.sent_count(), 81);
    }

    #[test]
    fn diagonal_crossing_adds_seventeen_without_duplicates() {
        let mut view = ChunkView::new(ChunkPos(0, 0));
        view.plan(ChunkPos(0, 0));
        let u = view.plan(ChunkPos(1, 1));
        assert_eq!(u.add.len(), 17, "new east column 9 + new south row 8");
        assert_eq!(u.remove.len(), 17);
        let mut add = u.add.clone();
        add.sort_by_key(|p| (p.0, p.1));
        add.dedup();
        assert_eq!(add.len(), 17);
        assert_eq!(view.sent_count(), 81);
    }

    #[test]
    fn chunk_of_block_handles_negative_and_invalid() {
        assert_eq!(ChunkView::chunk_of_block(0.0, 0.0), Some(ChunkPos(0, 0)));
        assert_eq!(ChunkView::chunk_of_block(15.9, 16.0), Some(ChunkPos(0, 1)));
        assert_eq!(ChunkView::chunk_of_block(-0.1, -16.1), Some(ChunkPos(-1, -2)));
        assert_eq!(ChunkView::chunk_of_block(-16.0, 15.999), Some(ChunkPos(-1, 0)));
        assert_eq!(ChunkView::chunk_of_block(f64::NAN, 0.0), None);
        assert_eq!(ChunkView::chunk_of_block(0.0, f64::INFINITY), None);
        // 越界 clamp 后仍是有限区块坐标（不 panic、不溢出）。
        assert!(ChunkView::chunk_of_block(-1.0e30, 1.0e30).is_some());
    }
}
