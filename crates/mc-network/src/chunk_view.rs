//! chunk_view: 玩家视距区块集合（已发送/待卸载）追踪，纯逻辑无 IO。
//! 26.3 客户端要求中心 section 的 4 正 + 4 斜邻居都存在才编译；视距半径来自
//! `ServerConfig.view_distance`，经 [`clamp_view_radius`] 限制在 2..=10。
//! `plan` 只在中心变化时调用，返回本批要发的区块（中心优先顺序，直接用于
//! ChunkBatchStart/Finished）和要发的 UnloadChunk。
use mc_core::ChunkPos;
use std::collections::HashSet;
use tracing::warn;

/// 旧入口默认的 Play 视距半径（切比雪夫）：81 块，兼容既有测试/旧调用点。
pub const PLAY_VIEW_RADIUS: i32 = 4;
/// `(2*4+1)^2`。
pub const PLAY_VIEW_CHUNK_COUNT: usize = 81;
/// 有效视距下限：客户端 `hasAllNeighbors` 至少需要 3×3（中心 + 8 邻居）。
pub const MIN_VIEW_RADIUS: i32 = 2;
/// 有效视距上限：控制初始 `(2r+1)^2` = 441 块的生成/发送开销。
pub const MAX_VIEW_RADIUS: i32 = 10;

/// 唯一的“有效视距”计算：越界 clamp 到 2..=10 并 warn。
/// 服务器配置、LoginPlay 下发、net 线程区块流、ECS 自检都必须走这里。
pub fn clamp_view_radius(radius: i32) -> i32 {
    if radius < MIN_VIEW_RADIUS {
        warn!("view_distance {radius} < {MIN_VIEW_RADIUS}, clamped to {MIN_VIEW_RADIUS}");
        MIN_VIEW_RADIUS
    } else if radius > MAX_VIEW_RADIUS {
        warn!("view_distance {radius} > {MAX_VIEW_RADIUS}, clamped to {MAX_VIEW_RADIUS}");
        MAX_VIEW_RADIUS
    } else {
        radius
    }
}

/// 有效视距对应的区块数 `(2r+1)^2`（r 先 clamp）。
pub fn view_chunk_count(radius: i32) -> usize {
    let side = (clamp_view_radius(radius) as usize) * 2 + 1;
    side * side
}

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
    radius: i32,
    sent: HashSet<ChunkPos>,
}

impl ChunkView {
    /// 按有效视距创建；`radius` 越界时 clamp（见 [`clamp_view_radius`]）。
    pub fn new(center: ChunkPos, radius: i32) -> Self {
        Self { center, radius: clamp_view_radius(radius), sent: HashSet::new() }
    }

    pub fn center(&self) -> ChunkPos {
        self.center
    }

    /// 有效视距半径（已 clamp）。
    pub fn radius(&self) -> i32 {
        self.radius
    }

    pub fn sent_count(&self) -> usize {
        self.sent.len()
    }

    pub fn sent_contains(&self, pos: ChunkPos) -> bool {
        self.sent.contains(&pos)
    }

    /// 以 `new_center` 重新规划视距：更新内部已发送集合，并返回本次要发/要卸的区块。
    /// 初始调用（sent 为空）返回全部 `(2r+1)^2` 块；同一中心重复调用返回空变更。
    pub fn plan(&mut self, new_center: ChunkPos) -> ChunkViewUpdate {
        self.center = new_center;
        let desired_list = mc_world::view_chunks(new_center, self.radius);
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
    fn clamp_boundaries_and_chunk_counts() {
        assert_eq!(clamp_view_radius(-1), MIN_VIEW_RADIUS);
        assert_eq!(clamp_view_radius(1), 2);
        assert_eq!(clamp_view_radius(2), 2);
        assert_eq!(clamp_view_radius(4), 4);
        assert_eq!(clamp_view_radius(10), 10);
        assert_eq!(clamp_view_radius(11), 10);
        assert_eq!(clamp_view_radius(i32::MAX), 10);
        assert_eq!(view_chunk_count(2), 25);
        assert_eq!(view_chunk_count(4), PLAY_VIEW_CHUNK_COUNT);
        assert_eq!(view_chunk_count(10), 441);
        assert_eq!(view_chunk_count(1), 25, "count uses the clamped radius");
        assert_eq!(view_chunk_count(99), 441);
    }

    #[test]
    fn initial_plan_is_81_center_first() {
        let center = ChunkPos(23, 23);
        let mut view = ChunkView::new(center, PLAY_VIEW_RADIUS);
        assert_eq!(view.radius(), 4);
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
    fn radius_two_and_ten_counts_are_center_first_without_duplicates() {
        for (radius, count) in [(2, 25), (4, 81), (10, 441)] {
            let center = ChunkPos(-3, 7);
            let mut view = ChunkView::new(center, radius);
            assert_eq!(view.radius(), radius);
            let u = view.plan(center);
            assert_eq!(u.add.len(), count, "radius={radius}");
            assert_eq!(u.add[0], center, "radius={radius} center first");
            assert!(u.remove.is_empty());
            let mut sorted = u.add.clone();
            sorted.sort_by_key(|p| (p.0, p.1));
            sorted.dedup();
            assert_eq!(sorted.len(), count, "radius={radius} no duplicates");
            for p in &u.add {
                assert!(
                    (p.0 - center.0).abs() <= radius && (p.1 - center.1).abs() <= radius,
                    "radius={radius} chunk {p:?} outside view"
                );
            }
        }
    }

    #[test]
    fn crossing_one_chunk_adds_nine_removes_nine_without_duplicates() {
        let mut view = ChunkView::new(ChunkPos(23, 23), PLAY_VIEW_RADIUS);
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
    fn radius_two_crossing_adds_five_removes_five() {
        let mut view = ChunkView::new(ChunkPos(23, 23), 2);
        view.plan(ChunkPos(23, 23));
        let u = view.plan(ChunkPos(24, 23));
        assert_eq!(u.add.len(), 5);
        assert_eq!(u.remove.len(), 5);
        assert!(u.add.iter().all(|p| p.0 == 26 && (21..=25).contains(&p.1)));
        assert!(u.remove.iter().all(|p| p.0 == 21 && (21..=25).contains(&p.1)));
        assert_eq!(view.sent_count(), 25);
        assert!(view.plan(ChunkPos(24, 23)).is_empty());
    }

    #[test]
    fn radius_ten_crossing_adds_twenty_one_removes_twenty_one() {
        let mut view = ChunkView::new(ChunkPos(0, 0), 10);
        view.plan(ChunkPos(0, 0));
        let u = view.plan(ChunkPos(1, 0));
        assert_eq!(u.add.len(), 21);
        assert_eq!(u.remove.len(), 21);
        assert!(u.add.iter().all(|p| p.0 == 11 && (-10..=10).contains(&p.1)));
        assert!(u.remove.iter().all(|p| p.0 == -10 && (-10..=10).contains(&p.1)));
        assert_eq!(view.sent_count(), 441);
        assert!(view.plan(ChunkPos(1, 0)).is_empty());
    }

    #[test]
    fn zigzag_reuses_previous_chunks() {
        let mut view = ChunkView::new(ChunkPos(0, 0), PLAY_VIEW_RADIUS);
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
        let mut view = ChunkView::new(ChunkPos(0, 0), PLAY_VIEW_RADIUS);
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
