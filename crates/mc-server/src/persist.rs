//! persist: 可变世界刷盘（P17 挖掘/放置落盘），只兼容 26.3/777。
//!
//! 触发时机：
//! - 每 1200 tick（60s）且有脏 chunk 时刷一次；
//! - 有玩家断线待删除（`PendingPlayerRemoval` 非空）且有脏 chunk 时立即刷；
//! - 进程退出时由 `main` 全量刷一次（见 `main.rs`；ECS tick 循环外）。
//!
//! 存档格式见 `mc_world::MutableWorld::save_to_dir`：自有稀疏格式
//! `world/overrides-<seed>.dat`（与 Anvil 的差距见 HANDOFF §5）。
//! 存档按 seed 隔离：`seed` 变更后旧文件保留、新 seed 用新文件并 warn。
use bevy_ecs::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

use crate::movement::PendingPlayerRemoval;
use crate::resources::TickCounter;

/// net 线程与 ECS 共享的权威可变世界（同一 `Arc`，见 `main.rs`）。
#[derive(Resource, Clone, Debug)]
pub struct SharedWorld(pub Arc<mc_world::MutableWorld>);

/// 存档目录（`server.toml` 的 `world_dir`，默认 `world/`）。
#[derive(Resource, Clone, Debug)]
pub struct WorldSaveDir(pub PathBuf);

/// redb 模式共享的存储库；`None`/资源缺失 = 旧稀疏档模式（旧调用方行为不变）。
#[derive(Resource, Clone, Debug)]
pub struct SharedStore(pub Option<Arc<mc_world::store::WorldStore>>);

/// redb 模式 60s 周期刷背包/XP 用（与 net 线程共享同一份 DropState）。
#[derive(Resource, Clone, Debug)]
pub struct SharedDropState(pub Arc<mc_network::DropState>);

/// 每 1200 tick 刷盘一次（60s）或断线时刷脏 chunk。
/// redb 模式：同一触发点把所有在线背包/XP 与脏方块提交（方块 + journal 一个事务）。
/// 旧模式：原 `MutableWorld::save_to_dir` 路径逐字节不变。
/// 注意：调用方必须在 `player_removal_system` 之前调度，否则 pending 已被 drain、
/// 断线即刷永远看不到非空队列（见 `main.rs` chain 顺序注释）。
pub fn autosave_system(
    tick: Res<TickCounter>,
    world: Res<SharedWorld>,
    dir: Res<WorldSaveDir>,
    pending: Res<PendingPlayerRemoval>,
    store: Option<Res<SharedStore>>,
    drops: Option<Res<SharedDropState>>,
) {
    let periodic = tick.0 % 1200 == 0;
    let on_disconnect = !pending.is_empty();
    if !periodic && !on_disconnect {
        return;
    }
    if let Some(store) = store.as_ref().and_then(|s| s.0.as_ref()) {
        let _ = store;
        if let Some(drops) = &drops {
            drops.0.save_all_inventories();
        }
        match world.0.flush_to_store(tick.0) {
            Ok(n) => info!(
                "world saved to redb seed={} edits={} (periodic={periodic} on_disconnect={on_disconnect} tick={})",
                world.0.seed(), n, tick.0
            ),
            Err(e) => warn!("world save to redb failed seed={} ({e})", world.0.seed()),
        }
        return;
    }
    if world.0.dirty_len() == 0 {
        return;
    }
    match world.0.save_to_dir(&dir.0) {
        Ok(n) => info!(
            "world saved dir={} seed={} overrides={} (periodic={periodic} on_disconnect={on_disconnect} tick={})",
            dir.0.display(),
            world.0.seed(),
            n,
            tick.0
        ),
        Err(e) => warn!("world save failed dir={} ({e})", dir.0.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_world::{AIR_BLOCK_ID, STONE_BLOCK_ID};

    fn world_with_edit() -> SharedWorld {
        let w = Arc::new(mc_world::MutableWorld::new(42));
        w.set(10, 64, 10, STONE_BLOCK_ID);
        SharedWorld(w)
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::env::temp_dir().join(format!("mc-server-persist-{}-{tag}-{n}", std::process::id()))
    }

    fn schedule() -> Schedule {
        let mut sched = Schedule::default();
        sched.add_systems(autosave_system);
        sched
    }

    #[test]
    fn disconnect_with_dirty_flushes_and_clears_dirty() {
        let mut world = World::new();
        let dir = tmp_dir("disconnect");
        world.insert_resource(TickCounter(7));
        world.insert_resource(world_with_edit());
        world.insert_resource(WorldSaveDir(dir.clone()));
        let mut pending = PendingPlayerRemoval::default();
        pending.push(99);
        world.insert_resource(pending);
        schedule().run(&mut world);
        let path = mc_world::MutableWorld::save_path(&dir, 42);
        assert!(path.exists(), "disconnect must flush dirty chunks");
        let back = mc_world::MutableWorld::load_from_dir(&dir, 42).unwrap();
        assert_eq!(back.get(10, 64, 10), STONE_BLOCK_ID);
        assert_eq!(
            world.get_resource::<SharedWorld>().unwrap().0.dirty_len(),
            0,
            "successful save must clear dirty"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_world_never_touches_disk() {
        let mut world = World::new();
        let dir = tmp_dir("clean");
        world.insert_resource(TickCounter(1200));
        world.insert_resource(SharedWorld(Arc::new(mc_world::MutableWorld::new(42))));
        world.insert_resource(WorldSaveDir(dir.clone()));
        world.insert_resource(PendingPlayerRemoval::default());
        schedule().run(&mut world);
        assert!(
            !mc_world::MutableWorld::save_path(&dir, 42).exists(),
            "no dirty chunks must not create save files"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn periodic_tick_flushes_without_disconnect() {
        let mut world = World::new();
        let dir = tmp_dir("periodic");
        world.insert_resource(TickCounter(1200));
        world.insert_resource(world_with_edit());
        world.insert_resource(WorldSaveDir(dir.clone()));
        world.insert_resource(PendingPlayerRemoval::default());
        schedule().run(&mut world);
        assert!(mc_world::MutableWorld::save_path(&dir, 42).exists());
        // 非周期 tick 且无断线：不刷。
        let w2 = Arc::new(mc_world::MutableWorld::new(43));
        w2.set(10, 64, 10, AIR_BLOCK_ID);
        world.insert_resource(SharedWorld(w2));
        world.insert_resource(TickCounter(7));
        schedule().run(&mut world);
        assert!(
            !mc_world::MutableWorld::save_path(&dir, 43).exists(),
            "non-periodic tick without disconnect must not save"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P36.2：ECS 触发点在 redb 模式写 state-<seed>.redb 并清 dirty，不产生旧 .dat。
    #[test]
    fn redb_mode_autosave_writes_store_not_legacy_file() {
        use mc_world::store::WorldStore;
        let mut world = World::new();
        let dir = tmp_dir("redb");
        let store = Arc::new(WorldStore::open(&dir, 42).unwrap());
        let shared = Arc::new(mc_world::MutableWorld::with_store(42, Arc::clone(&store)).unwrap());
        shared.set_with_context(10, 100, 10, STONE_BLOCK_ID, 5, 6);
        world.insert_resource(TickCounter(1200));
        world.insert_resource(SharedWorld(Arc::clone(&shared)));
        world.insert_resource(WorldSaveDir(dir.clone()));
        world.insert_resource(PendingPlayerRemoval::default());
        world.insert_resource(SharedStore(Some(Arc::clone(&store))));
        // 不注入 SharedDropState：本单测只验方块+journal 的 ECS 接线。
        schedule().run(&mut world);
        assert_eq!(store.block(10, 100, 10).unwrap(), Some(STONE_BLOCK_ID));
        assert_eq!(store.replay().unwrap()[0].actor, 5);
        assert_eq!(store.replay().unwrap()[0].tick, 6);
        assert_eq!(shared.dirty_len(), 0);
        assert!(!mc_world::MutableWorld::save_path(&dir, 42).exists());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
