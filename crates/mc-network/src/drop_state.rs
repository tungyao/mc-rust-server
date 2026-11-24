//! drop_state: 掉落物实体 + 背包的 net 线程共享状态，只兼容 26.3/777。
//!
//! 设计（P17 模式的延续）：socket 仍在 net 线程，ECS 没有写通道，因此掉落物的
//! 生成/同步/拾取判定与背包同步都在 net 线程完成（权威逻辑见
//! `mc_world::{ItemEntities, PlayerInventory}` 纯逻辑模块，本文件只做
//! 并发容器 + 存档 + 发包所需的派生状态）。
//!
//! - `items`：全服唯一的 `ItemEntities`（entity_id 全局自增，`next_id` 从 1000 起，
//!   与 LoginPlay entity_id=1 不冲突，永不复用，重启后从 1000 重新开始）。
//! - `inventories`：conn_id → 背包；Join 时按用户名读盘（缺文件=新背包+石头开局），
//!   拾取/放置/断线时刷盘（`player-<user>-<seed>.dat`，seed 隔离；键用用户名而非 uuid，
//!   因离线模式 uuid 每次随机，见 `mc_world::inventory` 文档）。
//! - `known`：conn_id → 该连接已见实体（视距过滤：离开视距发 Remove，进入补发）。

use mc_world::{ItemEntities, PlayerInventory};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    Mutex,
    atomic::{AtomicI32, Ordering},
};

/// 掉落物 entity_id 起始（避开 LoginPlay entity_id=1）。
pub const FIRST_DROP_ENTITY_ID: i32 = 1000;
/// 拾取动画 collector（本服 LoginPlay entity_id 恒为 1）。
pub const PLAYER_ENTITY_ID: i32 = 1;
/// 新背包开局石头数量（ hotbar 槽 36），保证“放置扣 1”可验证；后续按生存模式替换。
pub const STARTER_STONE_COUNT: i32 = mc_world::inventory::STARTER_COUNT;

/// net 线程共享的掉落/背包状态（`Arc` 进 `PlayOptions`）。
#[derive(Debug)]
pub struct DropState {
    pub items: Mutex<ItemEntities>,
    inventories: Mutex<HashMap<u64, PlayerInventory>>,
    known: Mutex<HashMap<u64, HashSet<i32>>>,
    next_id: AtomicI32,
    save_dir: PathBuf,
    seed: i64,
}

impl DropState {
    pub fn new(save_dir: PathBuf, seed: i64) -> Self {
        Self {
            items: Mutex::new(ItemEntities::new()),
            inventories: Mutex::new(HashMap::new()),
            known: Mutex::new(HashMap::new()),
            next_id: AtomicI32::new(FIRST_DROP_ENTITY_ID),
            save_dir,
            seed,
        }
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// 分配全局唯一 entity_id（永不复用）。
    pub fn alloc_id(&self) -> i32 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    fn lock_invs(&self) -> std::sync::MutexGuard<'_, HashMap<u64, PlayerInventory>> {
        self.inventories.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn lock_known(&self) -> std::sync::MutexGuard<'_, HashMap<u64, HashSet<i32>>> {
        self.known.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Join 时调入背包：读盘命中则用盘档，否则新背包 + 石头开局（后续替换为生存模式）。
    pub fn load_inventory(&self, conn_id: u64, user_tag: &str) -> (usize, u8) {
        let (mut inv, is_new) =
            PlayerInventory::load_from_dir(&self.save_dir, user_tag, self.seed).unwrap_or_else(
                |_| (PlayerInventory::new(), true),
            );
        if is_new && inv.is_empty() {
            inv.slots_hotbar_starter();
        }
        let held = inv.selected();
        let filled = inv.filled_count();
        self.lock_invs().insert(conn_id, inv);
        self.lock_known().insert(conn_id, HashSet::new());
        let _ = (filled, held);
        (filled, held)
    }

    /// 断线时刷盘 + 释放连接状态（known/背包内存）。
    pub fn unload_inventory(&self, conn_id: u64, user_tag: &str) {
        if let Some(inv) = self.lock_invs().remove(&conn_id) {
            let _ = inv.save_to_dir(&self.save_dir, user_tag, self.seed);
        }
        self.lock_known().remove(&conn_id);
    }

    /// 背包刷盘（拾取/放置后调用）。
    pub fn save_inventory(&self, conn_id: u64, user_tag: &str) {
        if let Some(inv) = self.lock_invs().get(&conn_id) {
            let _ = inv.save_to_dir(&self.save_dir, user_tag, self.seed);
        }
    }

    /// 对背包加独占锁执行闭包（调用方负责发包/刷盘）。
    pub fn with_inventory<R>(&self, conn_id: u64, f: impl FnOnce(&mut PlayerInventory) -> R) -> Option<R> {
        self.lock_invs().get_mut(&conn_id).map(f)
    }

    pub fn mark_known(&self, conn_id: u64, id: i32) {
        self.lock_known().entry(conn_id).or_default().insert(id);
    }

    pub fn unmark_known(&self, conn_id: u64, id: i32) {
        if let Some(set) = self.lock_known().get_mut(&conn_id) {
            set.remove(&id);
        }
    }

    pub fn is_known(&self, conn_id: u64, id: i32) -> bool {
        self.lock_known().get(&conn_id).map(|s| s.contains(&id)).unwrap_or(false)
    }

    pub fn known_ids(&self, conn_id: u64) -> Vec<i32> {
        self.lock_known().get(&conn_id).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_ids_are_unique_and_above_player() {
        let st = DropState::new(std::env::temp_dir(), 1);
        let a = st.alloc_id();
        let b = st.alloc_id();
        assert!(a >= FIRST_DROP_ENTITY_ID && b == a + 1 && a > PLAYER_ENTITY_ID);
    }

    #[test]
    fn inventory_load_unload_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mc-p18-drop-{}-rt", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let st = DropState::new(dir.clone(), 42);
        st.load_inventory(7, "some-uuid");
        let held = st.with_inventory(7, |inv| {
            inv.slots_hotbar_starter();
            inv.selected()
        });
        assert_eq!(held, Some(0));
        st.save_inventory(7, "some-uuid");
        assert!(PlayerInventory::save_path(&dir, "some-uuid", 42).exists());
        st.unload_inventory(7, "some-uuid");
        assert!(st.with_inventory(7, |_| ()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn known_tracking() {
        let st = DropState::new(std::env::temp_dir(), 1);
        st.load_inventory(9, "u");
        assert!(!st.is_known(9, 1001));
        st.mark_known(9, 1001);
        assert!(st.is_known(9, 1001));
        st.unmark_known(9, 1001);
        assert!(!st.is_known(9, 1001));
    }
}
