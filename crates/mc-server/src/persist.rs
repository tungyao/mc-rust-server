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

/// 每 1200 tick 刷盘一次（60s）或断线时刷脏 chunk。
/// 无脏 chunk 时直接返回，不碰磁盘。
/// 注意：调用方必须在 `player_removal_system` 之前调度，否则 pending 已被 drain、
/// 断线即刷永远看不到非空队列（见 `main.rs` chain 顺序注释）。
pub fn autosave_system(
    tick: Res<TickCounter>,
    world: Res<SharedWorld>,
    dir: Res<WorldSaveDir>,
    pending: Res<PendingPlayerRemoval>,
) {
    if world.0.dirty_len() == 0 {
        return;
    }
    let periodic = tick.0 % 1200 == 0;
    let on_disconnect = !pending.is_empty();
    if !periodic && !on_disconnect {
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
}
