//! craft9: P47 工作台 3×3 会话的权威 ClickContainer 处理（不碰玩家背包窗口 0）。
//!
//! 窗口 1 = 工作台：槽 0 结果、1..=9 合成格、10..=45 玩家背包（main/hotbar）。
//! 所有点击经 C2S ContainerClick(0x12) 解析结果进入这里，服务端权威修改后发全量
//! ContainerSetContent 回滚客户端预测。**不开放 `open_screen`**：26.3 数据包表里没有
//! menu type 注册表（见 data/manifest.json），包体 menu id 无生成表可查，按“不猜字节”
//! 约束不发该包；因此这是服务端权威会话 + 脚本/管理面板可驱动的 MVP（见 HANDOFF）。

use crate::drop_state::{Craft9Session, DropState};
use mc_protocol::packets;
use mc_world::crafting::{self, CRAFT9_RESULT_SLOT, CRAFT9_WINDOW_ID};

/// 窗口槽 → 玩家背包槽（10..=36 主包 9..=35；37..=45 快捷栏 36..=44）。
fn inv_slot(window_slot: i16) -> Option<i16> {
    match window_slot {
        10..=36 => Some(window_slot - 1),
        37..=45 => Some(window_slot - 1),
        _ => None,
    }
}

/// PICKUP/右键放一：与 `PlayerInventory::click_pickup` 同语义，但作用在会话合成格上。
fn pickup(slot: &mut Option<(i32, i32)>, cursor: &mut Option<(i32, i32)>, button: i8) -> bool {
    if button != 0 && button != 1 {
        return false;
    }
    if button == 0 {
        match (*slot, *cursor) {
            (None, None) => false,
            (Some(s), None) => {
                *cursor = Some(s);
                *slot = None;
                true
            }
            (None, Some(c)) => {
                *slot = Some(c);
                *cursor = None;
                true
            }
            (Some((aid, ac)), Some((bid, bc))) if aid == bid => {
                let room = mc_world::items::MAX_STACK - ac;
                if room <= 0 {
                    return false;
                }
                let take = room.min(bc);
                *slot = Some((aid, ac + take));
                *cursor = if bc > take { Some((bid, bc - take)) } else { None };
                true
            }
            (a, b) => {
                *slot = b;
                *cursor = a;
                true
            }
        }
    } else {
        match (*slot, *cursor) {
            (None, Some((id, count))) => {
                *slot = Some((id, 1));
                *cursor = if count > 1 { Some((id, count - 1)) } else { None };
                true
            }
            (Some((id, count)), None) => {
                let take = (count + 1) / 2;
                *cursor = Some((id, take));
                *slot = if count > take { Some((id, count - take)) } else { None };
                true
            }
            (Some((id, ac)), Some((cid, cc))) if id == cid && ac < mc_world::items::MAX_STACK => {
                *slot = Some((id, ac + 1));
                *cursor = if cc > 1 { Some((cid, cc - 1)) } else { None };
                true
            }
            _ => false,
        }
    }
}

/// SHIFT 整箱整理：从背包补一格原料到空合成格（只搬配方表内物品，不搬产物/杂项）。
/// 一次只补一格，调用方每合成一次调一次，直到背包没有可用原料。
fn refill_one_from_inventory(ds: &DropState, conn: u64, session: &mut Craft9Session) -> bool {
    for idx in 0..mc_world::crafting::GRID9_SIZE {
        if session.grid[idx].is_some() {
            continue;
        }
        let filled = ds.with_inventory(conn, |inv| {
            for slot in mc_world::inventory::MAIN_START..=45 {
                let Some((id, count)) = inv.get(slot as usize) else { continue };
                let used_in_recipe = crafting::RECIPES
                    .iter()
                    .any(|r| r.needs.iter().any(|(need, _)| *need == id));
                if !used_in_recipe {
                    continue;
                }
                let take = count.min(9);
                inv.set_slot(
                    slot as usize,
                    if count > take { Some((id, count - take)) } else { None },
                );
                return Some((id, take));
            }
            None
        });
        if let Some(Some(stack)) = filled {
            session.grid[idx] = Some(stack);
            return true;
        }
    }
    false
}

/// 结果槽：取一份结果到光标（PICKUP），或 SHIFT 连续合成直到背包满/原料不够（整箱整理）。
fn click_result(ds: &DropState, conn: u64, session: &mut Craft9Session, click: &packets::ContainerClick) {
    if click.click_type == packets::CLICK_QUICK_MOVE {
        // 空工作台按 SHIFT 时先从背包补料；每次都先确认能完整放下再扣原料，绝不吞物品。
        refill_one_from_inventory(ds, conn, session);
        loop {
            let mut grid = session.grid;
            let done = ds.with_inventory(conn, |inv| {
                let Some(recipe) = crafting::match_recipe_9(grid) else { return None };
                let (id, count) = recipe.result;
                let free = inv.free_slots_for(id, count);
                if free < count {
                    return None;
                }
                if !crafting::consume_grid9(&mut grid, recipe) {
                    return None;
                }
                assert_eq!(inv.add_item(id, count), 0, "pre-checked capacity must fit");
                Some(())
            });
            if done.flatten().is_some() {
                session.grid = grid;
                if !refill_one_from_inventory(ds, conn, session) {
                    break;
                }
            } else {
                break;
            }
        }
        return;
    }
    if click.click_type != packets::CLICK_PICKUP || (click.button != 0 && click.button != 1) {
        return;
    }
    let mut grid = session.grid;
    let outcome = ds.with_inventory(conn, |inv| {
        let recipe = crafting::match_recipe_9(grid)?;
        let (id, count) = recipe.result;
        match inv.cursor() {
            None => inv.set_cursor(Some((id, count))),
            Some((cid, cc)) if cid == id => {
                let take = (mc_world::items::MAX_STACK - cc).min(count);
                inv.set_cursor(Some((id, cc + take)));
                if count > take {
                    inv.add_item(id, count - take);
                }
            }
            _ => return None,
        }
        if crafting::consume_grid9(&mut grid, recipe) {
            Some(grid)
        } else {
            None
        }
    });
    if let Some(grid) = outcome.flatten() {
        session.grid = grid;
    }
}

/// 权威应用一次工作台点击；返回是否属于工作台会话（非会话点击不碰）。
pub fn apply_click(ds: &DropState, conn: u64, click: &packets::ContainerClick) -> bool {
    if click.container_id != CRAFT9_WINDOW_ID {
        return false;
    }
    let has_session = ds.craft9_session(conn).is_some();
    if !has_session {
        return false;
    }
    ds.with_craft9(conn, |session| {
        match click.slot {
            CRAFT9_RESULT_SLOT => click_result(ds, conn, session, click),
            1..=9 if click.click_type == packets::CLICK_PICKUP => {
                let idx = (click.slot - 1) as usize;
                // 合成格内容只在会话里；光标在背包里，先取会话格、再一起改背包光标。
                let mut slot = session.grid[idx];
                let mut cursor = ds.with_inventory(conn, |inv| inv.cursor()).unwrap_or(None);
                if pickup(&mut slot, &mut cursor, click.button) {
                    session.grid[idx] = slot;
                    ds.with_inventory(conn, |inv| inv.set_cursor(cursor));
                }
            }
            1..=9 if click.click_type == packets::CLICK_QUICK_MOVE => {
                let idx = (click.slot - 1) as usize;
                let Some(stack) = session.grid[idx] else { return };
                let leftover = ds
                    .with_inventory(conn, |inv| inv.add_item(stack.0, stack.1))
                    .unwrap_or(stack.1);
                session.grid[idx] = if leftover > 0 { Some((stack.0, leftover)) } else { None };
            }
            s if inv_slot(s).is_some() && click.click_type != packets::CLICK_THROW => {
                let slot = inv_slot(s).unwrap();
                ds.with_inventory(conn, |inv| {
                    inv.apply_click(slot, click.button, click.click_type);
                });
            }
            _ => {}
        }
    })
    .is_some()
}

/// 组工作台全量同步包（槽 0 结果由客户端按配方表自行计算，服务端只发输入/背包）。
pub fn build_craft9_content_body(ds: &DropState, conn: u64) -> Option<Vec<u8>> {
    let session = ds.craft9_session(conn)?;
    let (slots, carried, state) = ds.with_inventory(conn, |inv| {
        (inv.to_protocol_slots(), inv.cursor_slot(), inv.next_state())
    })?;
    let mut window_slots = vec![packets::Slot::empty(); 46];
    window_slots[0] = packets::Slot::empty();
    for (i, stack) in session.grid.into_iter().enumerate() {
        window_slots[1 + i] = match stack {
            Some((item_id, count)) => packets::Slot { item_id, count },
            None => packets::Slot::empty(),
        };
    }
    for i in 0..36 {
        window_slots[10 + i] = slots[9 + i];
    }
    let mut body = Vec::new();
    packets::SetContainerContent {
        window_id: CRAFT9_WINDOW_ID,
        state_id: state,
        slots: window_slots,
        carried,
    }
    .encode_body(&mut body);
    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn state() -> DropState {
        DropState::new(PathBuf::from(std::env::temp_dir().join(format!(
            "mc-craft9-ut-{}",
            std::process::id()
        ))), 777)
    }

    #[test]
    fn window_slot_mapping_matches_crafting_menu() {
        assert_eq!(inv_slot(10), Some(9));
        assert_eq!(inv_slot(36), Some(35));
        assert_eq!(inv_slot(37), Some(36));
        assert_eq!(inv_slot(45), Some(44));
        assert_eq!(inv_slot(9), None);
    }

    #[test]
    fn craft9_pickup_result_and_quick_move_are_authoritative() {
        let ds = state();
        ds.load_inventory(7, "C9");
        ds.with_inventory(7, |inv| {
            inv.set_slot(9, Some((mc_world::drops::ITEM_WHEAT, 9)));
        });
        ds.open_craft9(7, 0, 64, 0);
        // 从背包槽 9 拿起 9 小麦 → 右键放入工作台格 1 → 结果槽取干草捆。
        let click = |slot, button, ty| packets::ContainerClick {
            container_id: CRAFT9_WINDOW_ID,
            state_id: 0,
            slot,
            button,
            click_type: ty,
        };
        assert!(apply_click(&ds, 7, &click(10, 0, packets::CLICK_PICKUP)));
        assert!(apply_click(&ds, 7, &click(1, 0, packets::CLICK_PICKUP)));
        assert_eq!(ds.craft9_session(7).unwrap().grid[0], Some((mc_world::drops::ITEM_WHEAT, 9)));
        assert!(apply_click(&ds, 7, &click(0, 0, packets::CLICK_PICKUP)));
        let hay = crafting::RECIPES.iter().find(|r| r.name == "hay_block").unwrap().result.0;
        assert_eq!(ds.with_inventory(7, |inv| inv.cursor()), Some(Some((hay, 1))));
        assert!(ds.craft9_session(7).unwrap().grid.iter().all(|s| s.is_none()));
        // 光标放回背包后 shift 连续合成（先塞 9 组小麦进背包以验证整箱整理）。
        ds.with_inventory(7, |inv| {
            inv.set_cursor(None);
            for i in 0..9 {
                inv.set_slot(9 + i, Some((mc_world::drops::ITEM_WHEAT, 9)));
            }
        });
        // 背包里 9 组小麦；SHIFT 结果槽应从背包逐组补料 → 连做 9 个干草捆。
        assert!(apply_click(&ds, 7, &click(0, 0, packets::CLICK_QUICK_MOVE)));
        let total = ds.with_inventory(7, |inv| {
            inv.to_protocol_slots().iter().filter(|s| s.item_id == hay).map(|s| s.count).sum::<i32>()
        }).unwrap();
        assert_eq!(total, 9, "SHIFT must craft and stow every complete set");
        let _ = std::fs::remove_dir_all(ds.save_dir());
    }

    /// P54 A8：断开写 redb `block_entities`，重进 Join 恢复 10 槽快照（不改包流）。
    #[test]
    fn craft9_session_survives_disconnect_and_reconnect() {
        use mc_world::store::{craft_container_key, WorldStore};
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!(
            "mc-craft9-persist-{}-{}",
            std::process::id(),
            mc_world::crafting::GRID9_SIZE
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let seed = 777;
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let ds = DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store)));
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let pos = (12, 71, -8);
        ds.load_inventory_with_uuid(7, "PersistCrafter", uuid);
        ds.open_craft9(7, pos.0, pos.1, pos.2);
        ds.with_craft9(7, |s| s.grid[0] = Some((42, 9)));
        ds.unload_inventory(7, "PersistCrafter");
        assert!(ds.craft9_session(7).is_none());

        let key = craft_container_key(uuid, pos);
        let snap = store.load_craft_snapshot(&key).unwrap().unwrap();
        assert_eq!(snap.owner, uuid);
        assert!(snap.last_saved > 0);
        assert_eq!(snap.grid[1], Some((42, 9)));

        // 新连接 Join 恢复会话（无 open_screen/0x12 补充包，只在内存里恢复）。
        ds.load_inventory_with_uuid(8, "PersistCrafter", uuid);
        let restored = ds.craft9_session(8).unwrap();
        assert_eq!((restored.x, restored.y, restored.z), pos);
        assert_eq!(restored.grid[0], Some((42, 9)));
        assert_eq!(ds.craft9_persisted_info(uuid).0, true);
        assert!(ds.clear_craft9(8, uuid));
        assert_eq!(ds.craft9_persisted_info(uuid).0, false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_workbench_clicks_leave_session_untouched() {
        let ds = state();
        ds.load_inventory(8, "C9B");
        ds.open_craft9(8, 1, 64, 1);
        let click = packets::ContainerClick {
            container_id: 0,
            state_id: 0,
            slot: 1,
            button: 0,
            click_type: packets::CLICK_PICKUP,
        };
        assert!(!apply_click(&ds, 8, &click), "player window must stay 2×2");
        assert!(ds.craft9_session(8).unwrap().grid.iter().all(|s| s.is_none()));
        let _ = std::fs::remove_dir_all(ds.save_dir());
    }
}
