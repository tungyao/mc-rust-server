//! craft9: P47 工作台 3×3 会话的权威 ClickContainer 处理（不碰玩家背包窗口 0）。
//!
//! 窗口 1 = 工作台：槽 0 结果、1..=9 合成格、10..=45 玩家背包（main/hotbar）。
//! 所有点击经 C2S ContainerClick(0x12) 解析结果进入这里，服务端权威修改后发全量
//! ContainerSetContent 回滚客户端预测。
//! P55 B6 起右键工作台恒发 Open Screen(0x3c) + 全量 Content(0x12)（GUI 写死常开）。
//! `ClientboundOpenScreenPacket.STREAM_CODEC` 仅 CONTAINER_ID + menu VarInt + 标题组件，
//! `MenuType.CRAFTING = 12`，无名 StringTag 标题（真机 `10 bytes extra` 已校准）。
//! P56 按 jar `CraftingMenu.quickMoveStack` / `AbstractContainerMenu.doClick` 对齐
//! SHIFT 搬运与 state id 口径。

use crate::broadcast::QueuedPacket;
use crate::drop_state::{Craft9Session, DropState};
use mc_protocol::packets;
use mc_world::crafting::{self, CRAFT9_RESULT_SLOT, CRAFT9_WINDOW_ID};

/// P55：工作台界面标题。纯文本 → 无名 NBT StringTag payload（真机已校准）。
pub const WORKBENCH_GUI_TITLE: &str = "Crafting";

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
        if let Some(stack) = take_from_inventory(ds, conn, |id| {
            crafting::RECIPES.iter().any(|r| crafting::recipe_accepts_ingredient(r, id))
        }) {
            session.grid[idx] = Some(stack);
            return true;
        }
    }
    false
}

/// P60 shape-aware 补料：只填锁定配方的形状空格。
/// - shaped：找首个一致的放置（非覆盖格须空；覆盖格空着待补或放着可接受的料），
///   把空格按格填充（每格最多 9 个，背包槽序确定性）。
/// - shapeless：已占格须全可接受（否则停，料不乱动），填首个空格（最多 9 个）。
/// 返回是否填了 ≥1 格。物品只在格子/背包之间搬运，不丢失。
fn refill_for_recipe(
    ds: &DropState,
    conn: u64,
    session: &mut Craft9Session,
    recipe: &crafting::Recipe,
) -> bool {
    if recipe.shaped {
        let Some(place) = crafting::refill_placement(recipe, &session.grid, 3) else {
            return false;
        };
        let mut filled = false;
        for (idx, need) in place {
            if session.grid[idx].is_some() {
                continue;
            }
            if let Some(stack) =
                take_from_inventory(ds, conn, |id| crafting::need_accepts(need, id))
            {
                session.grid[idx] = Some(stack);
                filled = true;
            }
        }
        filled
    } else {
        for s in session.grid.iter().flatten() {
            if !crafting::recipe_accepts_ingredient(recipe, s.0) {
                return false;
            }
        }
        let Some(idx) = session.grid.iter().position(|s| s.is_none()) else {
            return false;
        };
        if let Some(stack) = take_from_inventory(ds, conn, |id| {
            crafting::recipe_accepts_ingredient(recipe, id)
        }) {
            session.grid[idx] = Some(stack);
            true
        } else {
            false
        }
    }
}

/// 从背包按槽序取一叠（`accept` 过滤，单格最多 9 个），返回取出的栈。
fn take_from_inventory(
    ds: &DropState,
    conn: u64,
    accept: impl Fn(i32) -> bool,
) -> Option<(i32, i32)> {
    ds.with_inventory(conn, |inv| {
        for slot in mc_world::inventory::MAIN_START..=45 {
            let Some((id, count)) = inv.get(slot as usize) else {
                continue;
            };
            if !accept(id) {
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
    })
    .flatten()
}

/// P56 / 26.3 jar `CraftingMenu.quickMoveStack` 对窗口槽 10..=45（玩家背包）的 SHIFT：
/// 先 `moveItemStackTo(stack, 1, 10, false)` —— 同物品先合并、再填入空合成格；剩余留在
/// 背包槽，由调用方继续走 main<->hotbar 搬运。返回是否至少搬入一件。
fn shift_inventory_into_grid(
    ds: &DropState,
    conn: u64,
    session: &mut Craft9Session,
    window_slot: i16,
) -> bool {
    let raw = match inv_slot(window_slot) {
        Some(s) => s,
        None => return false,
    };
    let Some((id, count)) = ds.with_inventory(conn, |inv| inv.get(raw as usize)).unwrap_or(None) else {
        return false;
    };
    let mut left = count;
    for cell in session.grid.iter_mut() {
        if left <= 0 {
            break;
        }
        match cell {
            Some((cid, c)) if *cid == id && *c < mc_world::items::MAX_STACK => {
                let take = (mc_world::items::MAX_STACK - *c).min(left);
                *c += take;
                left -= take;
            }
            None => {
                let take = mc_world::items::MAX_STACK.min(left);
                *cell = Some((id, take));
                left -= take;
            }
            _ => {}
        }
    }
    if left == count {
        return false;
    }
    ds.with_inventory(conn, |inv| {
        inv.set_slot(raw as usize, if left > 0 { Some((id, left)) } else { None });
    });
    true
}

/// P60 拖拽分堆落子（窗口 1：合成格 1..=9 + 背包 10..=45；结果槽 0 不可进）。
/// 口径与 [`mc_world::inventory::PlayerInventory::apply_drag_end`] 一致
///（经 [`mc_world::inventory::PlayerInventory::drag_split`]）。
/// 任一槽非法则整单 no-op、光标不动，返回 false。
fn apply_drag_end_w1(
    ds: &DropState,
    conn: u64,
    session: &mut Craft9Session,
    cursor: &mut Option<(i32, i32)>,
    left: bool,
    slots: &[i16],
) -> bool {
    let Some((id, total)) = *cursor else {
        return false;
    };
    // 去重 + 预校验全部槽合法。
    let mut uniq: Vec<i16> = Vec::new();
    for s in slots {
        let ok = matches!(*s, 1..=9) || inv_slot(*s).is_some();
        if !ok {
            return false;
        }
        if !uniq.contains(s) {
            uniq.push(*s);
        }
    }
    if uniq.is_empty() {
        return false;
    }
    // 快照现有占用（合成格直读 + 背包经锁读一次）。
    let mut cur = Vec::with_capacity(uniq.len());
    for s in &uniq {
        if (1..=9).contains(s) {
            cur.push(session.grid[(*s - 1) as usize]);
        } else {
            let inv_idx = inv_slot(*s).unwrap() as usize;
            cur.push(ds.with_inventory(conn, |inv| inv.get(inv_idx)).unwrap_or(None));
        }
    }
    let Some((takes, rest)) =
        mc_world::inventory::PlayerInventory::drag_split(id, total, left, &cur)
    else {
        return false;
    };
    // 写回（合成格直写 + 背包经锁写）。
    for (k, s) in uniq.iter().enumerate() {
        let take = takes[k];
        if take <= 0 {
            continue;
        }
        if (1..=9).contains(s) {
            let idx = (*s - 1) as usize;
            match session.grid[idx] {
                Some((_, c)) => session.grid[idx] = Some((id, c + take)),
                None => session.grid[idx] = Some((id, take)),
            }
        } else {
            let inv_idx = inv_slot(*s).unwrap() as usize;
            ds.with_inventory(conn, |inv| {
                match inv.get(inv_idx) {
                    Some((_, c)) => inv.set_slot(inv_idx, Some((id, c + take))),
                    None => inv.set_slot(inv_idx, Some((id, take))),
                };
            });
        }
    }
    *cursor = if rest > 0 { Some((id, rest)) } else { None };
    true
}

/// P60 双击收集（窗口 1）：光标为空时先拿起点击槽（合成格 1..=9 整栈进光标，
/// 背包槽走背包 PICKUP 语义）；再把合成格全格 + 背包 9..=45 的同种收到光标
/// 直到满栈（结果槽 0 不动）。
fn collect_all_w1(
    ds: &DropState,
    conn: u64,
    session: &mut Craft9Session,
    clicked: i16,
) -> bool {
    let mut changed = false;
    if ds.with_inventory(conn, |inv| inv.cursor().is_none()).unwrap_or(false) {
        if (1..=9).contains(&clicked) {
            let idx = (clicked - 1) as usize;
            match session.grid[idx].take() {
                Some(stack) => {
                    ds.with_inventory(conn, |inv| inv.set_cursor(Some(stack)));
                    changed = true;
                }
                None => return false,
            }
        } else if let Some(inv_idx) = inv_slot(clicked) {
            let idx = inv_idx as usize;
            let got = ds.with_inventory(conn, |inv| {
                if inv.cursor().is_some() {
                    return None;
                }
                match inv.get(idx) {
                    Some(stack) => {
                        inv.set_slot(idx, None);
                        inv.set_cursor(Some(stack));
                        Some(())
                    }
                    None => None,
                }
            });
            if got.flatten().is_none() {
                return false;
            }
            changed = true;
        } else {
            return false;
        }
    }
    // 扫同种进光标至满（合成格 → 背包，槽序确定性；结果槽不动）。
    let cur = ds.with_inventory(conn, |inv| inv.cursor()).unwrap_or(None);
    let Some((id, mut total)) = cur else {
        return changed;
    };
    if total >= mc_world::items::MAX_STACK {
        return changed;
    }
    for idx in 0..mc_world::crafting::GRID9_SIZE {
        if total >= mc_world::items::MAX_STACK {
            break;
        }
        if let Some((cid, c)) = session.grid[idx] {
            if cid != id {
                continue;
            }
            let take = (mc_world::items::MAX_STACK - total).min(c);
            session.grid[idx] = if c > take { Some((id, c - take)) } else { None };
            total += take;
            changed = true;
        }
    }
    ds.with_inventory(conn, |inv| {
        for slot in mc_world::inventory::MAIN_START..=45 {
            if total >= mc_world::items::MAX_STACK {
                break;
            }
            if let Some((cid, c)) = inv.get(slot as usize) {
                if cid != id {
                    continue;
                }
                let take = (mc_world::items::MAX_STACK - total).min(c);
                inv.set_slot(
                    slot as usize,
                    if c > take { Some((id, c - take)) } else { None },
                );
                total += take;
                changed = true;
            }
        }
        inv.set_cursor(Some((id, total)));
    });
    changed
}

/// P60 工作台拖拽三段式：button 0/1/2 起拖（左/右/中；中键生存无操作），
/// 4/5/6 划过加槽，8/9/10 落子。窗口/模式不一致整单丢弃并清态。
fn handle_drag_w1(
    ds: &DropState,
    conn: u64,
    session: &mut Craft9Session,
    click: &packets::ContainerClick,
) {
    let header = click.button >> 2; // 0 起拖 / 1 加槽 / 2 落子
    let mode = click.button & 3; // 0 左 / 1 右 / 2 中
    match header {
        0 => {
            if mode == 2 {
                ds.drag_clear(conn);
                return;
            }
            // 起拖要求光标非空（空光标起拖无意义，清掉旧态）。
            let has_cursor =
                ds.with_inventory(conn, |inv| inv.cursor().is_some()).unwrap_or(false);
            if !has_cursor {
                ds.drag_clear(conn);
                return;
            }
            ds.drag_begin(conn, click.container_id, mode == 0);
        }
        1 => {
            if mode == 2 || !ds.drag_add(conn, click.container_id, click.slot) {
                ds.drag_clear(conn);
            }
        }
        2 => {
            let Some(state) = ds.drag_take(conn) else {
                return;
            };
            if state.window != click.container_id {
                return;
            }
            let mut cursor = ds.with_inventory(conn, |inv| inv.cursor()).unwrap_or(None);
            if apply_drag_end_w1(ds, conn, session, &mut cursor, state.left, &state.slots) {
                ds.with_inventory(conn, |inv| inv.set_cursor(cursor));
            }
        }
        _ => {
            ds.drag_clear(conn);
        }
    }
}

/// 结果槽：取一份结果到光标（PICKUP），或 SHIFT 连续合成直到背包满/原料不够（整箱整理）。
fn click_result(ds: &DropState, conn: u64, session: &mut Craft9Session, click: &packets::ContainerClick) {
    if click.click_type == packets::CLICK_QUICK_MOVE {
        // SHIFT 连点只重复当前锁定的配方（原版口径：只做当前展示的结果），
        // 避免可逆配方在"任意配方"循环里来回倒腾。
        // 格子已有配方直接锁定；空格则先把背包的料补满格（shape-aware 放在后面，
        // 这里只管搬运），再按补满后的格子锁定——避免半格时锁错小配方
        //（如补 3 格小麦就锁成面包，而不是 9 格的干草捆）。
        let locked = match crafting::match_recipe_9(session.grid) {
            Some(r) => Some(r),
            None => {
                for _ in 0..mc_world::crafting::GRID9_SIZE {
                    if session.grid.iter().all(|s| s.is_some()) {
                        break;
                    }
                    if !refill_one_from_inventory(ds, conn, session) {
                        break;
                    }
                }
                crafting::match_recipe_9(session.grid)
            }
        };
        let Some(recipe) = locked else { return };
        // 借表序指针的 'static 配方做循环锚点（Recipe: Copy 足够轻量也可直接复制引用）。
        let target_name = recipe.name;
        let mut rounds = 0;
        loop {
            rounds += 1;
            if rounds > 4096 {
                break; // 背包有限，正常到不了；防御性上限，防逻辑 bug 卡死 play 循环。
            }
            // 1) 能做就做（先确认背包放得下产物，绝不吞原料）。
            let mut grid = session.grid;
            let crafted = ds
                .with_inventory(conn, |inv| {
                    let want = crafting::RECIPES.iter().find(|r| r.name == target_name)?;
                    let (id, count) = want.result;
                    if inv.free_slots_for(id, count) < count {
                        return None;
                    }
                    if !crafting::consume_grid9(&mut grid, want) {
                        return None;
                    }
                    assert_eq!(inv.add_item(id, count), 0, "pre-checked capacity must fit");
                    Some(())
                })
                .flatten()
                .is_some();
            if crafted {
                session.grid = grid;
            }
            // 2) 补料（shape-aware，只填锁定配方的形状空格）；都没进展就停。
            // 料留在格子里（不丢失），与单次合成一致。
            let filled = refill_for_recipe(ds, conn, session, recipe);
            if !crafted && !filled {
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
        // P60 拖拽/收集/CLONE：拖拽走三段式序列；收集整单处理；CLONE 生存恒 no-op。
        // 非拖拽点击的清态由入口（mod.rs）统一做，这里只管本窗三段。
        if click.click_type == packets::CLICK_QUICK_CRAFT {
            handle_drag_w1(ds, conn, session, click);
            return;
        }
        if click.click_type == packets::CLICK_PICKUP_ALL {
            collect_all_w1(ds, conn, session, click.slot);
            return;
        }
        if click.click_type == packets::CLICK_CLONE {
            return; // 生存恒无操作（创造才给整组；本服生存写死）。
        }
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
                if click.click_type == packets::CLICK_QUICK_MOVE {
                    // P56/26.3 `CraftingMenu.quickMoveStack`：背包 SHIFT 优先补合成格。
                    shift_inventory_into_grid(ds, conn, session, s);
                }
                ds.with_inventory(conn, |inv| {
                    inv.apply_click(slot, click.button, click.click_type);
                });
            }
            _ => {}
        }
    })
    .is_some()
}

/// 组工作台全量同步包（0x12）。槽 0 结果按 26.3 `CraftingMenu.slotChangedCraftingGrid`
/// 由服务端算出；原版在格变化时单发 0x14 结果槽，我们统一用本全量包 + 新 state id
/// 覆盖同一状态；客户端以服务端包为准，不留预测分叉。
pub fn build_craft9_content_body(ds: &DropState, conn: u64) -> Option<Vec<u8>> {
    let session = ds.craft9_session(conn)?;
    let (slots, carried, state) = ds.with_inventory(conn, |inv| {
        (inv.to_protocol_slots(), inv.cursor_slot(), inv.next_state())
    })?;
    let mut window_slots = vec![packets::Slot::empty(); 46];
    // P56 / 26.3 `CraftingMenu.slotChangedCraftingGrid`：结果槽由服务端按 1..=9 合成格
    // 权威算出并同步给客户端（原来恒发空，真实客户端看不到可点的结果）。
    window_slots[CRAFT9_RESULT_SLOT as usize] = match crafting::match_recipe_9(session.grid) {
        Some(recipe) => packets::Slot { item_id: recipe.result.0, count: recipe.result.1 },
        None => packets::Slot::empty(),
    };
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

/// P56 / 26.3 jar `CraftingMenu.slotChangedCraftingGrid` + 26.3
/// `ClientboundContainerSetSlotPacket.STREAM_CODEC`（CONTAINER_ID + VarInt stateId +
/// Short slot + ItemStack）：结果槽单槽同步 0x14 包体。日常点击同步统一走 0x12，
/// 此处提供与 jar 对齐的单槽包体，供协议单测/未来按需单播，不改变既有包流。
pub fn build_result_slot_body(ds: &DropState, conn: u64) -> Option<Vec<u8>> {
    let result = match ds
        .craft9_session(conn)
        .and_then(|session| crafting::match_recipe_9(session.grid))
    {
        Some(recipe) => packets::Slot { item_id: recipe.result.0, count: recipe.result.1 },
        None => packets::Slot::empty(),
    };
    let state = ds.with_inventory(conn, |inv| inv.next_state())?;
    let mut body = Vec::new();
    packets::SetContainerSlot {
        window_id: CRAFT9_WINDOW_ID,
        state_id: state,
        slot: CRAFT9_RESULT_SLOT,
        data: result,
    }
    .encode_body(&mut body);
    Some(body)
}

/// P55：`Open Screen 0x3c` 包体（仅 window/menu/title 三个字段，无 menu 附加 NBT）。
/// 26.3 jar 已校准：`MenuType.CRAFTING = 12`；`menu_id` 可由配置覆盖做真机 A/B。
pub fn build_open_screen_body(menu_id: i32) -> Vec<u8> {
    let mut body = Vec::new();
    packets::OpenScreen {
        window_id: CRAFT9_WINDOW_ID,
        menu_type: menu_id,
        title: WORKBENCH_GUI_TITLE.into(),
    }
    .encode_body(&mut body);
    body
}

/// 打开工作台 GUI：登记会话 → 生成 `Open Screen(0x3c)` + `Content(0x12)` 两个原始包体。
/// 调用方按顺序写给自己连接（或经 hub 单播）。无在线背包时不建会话、不发任何包。
pub fn open_workbench_gui(
    ds: &DropState,
    conn: u64,
    x: i32,
    y: i32,
    z: i32,
    menu_id: i32,
) -> Option<[QueuedPacket; 2]> {
    if ds.with_inventory(conn, |_| ()).is_none() {
        return None;
    }
    ds.open_craft9(conn, x, y, z);
    let content = build_craft9_content_body(ds, conn)?;
    Some([
        QueuedPacket {
            id: packets::PLAY_OPEN_SCREEN_S2C_ID,
            body: build_open_screen_body(menu_id),
        },
        QueuedPacket {
            id: packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID,
            body: content,
        },
    ])
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

    /// P55 B6：0x3c 包体 + 46 槽映射的完整骨架断言（menu ID 仍是推导值）。
    #[test]
    fn open_workbench_gui_bodies_and_slot_mapping() {
        use mc_protocol::packets::{self, CRAFTING_MENU_TYPE_ID};
        let ds = state();
        ds.load_inventory(11, "GuiCrafter");
        ds.with_inventory(11, |inv| {
            inv.set_slot(9, Some((mc_world::drops::ITEM_WHEAT, 7)));
            inv.set_slot(44, Some((mc_world::drops::ITEM_STICK, 3)));
        });
        let packets_pair = open_workbench_gui(&ds, 11, 4, 71, -2, CRAFTING_MENU_TYPE_ID).unwrap();
        let (screen, content) = (&packets_pair[0], &packets_pair[1]);
        assert_eq!(screen.id, packets::PLAY_OPEN_SCREEN_S2C_ID);
        let (open, used) = packets::OpenScreen::decode_body(&screen.body).unwrap();
        assert_eq!(used, screen.body.len(), "no extra NBT after title");
        assert_eq!(open.window_id, CRAFT9_WINDOW_ID);
        assert_eq!(open.menu_type, 12);
        assert_eq!(open.title, WORKBENCH_GUI_TITLE);
        assert_eq!(content.id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
        let (c, _) = packets::SetContainerContent::decode_body(&content.body).unwrap();
        assert_eq!(c.window_id, CRAFT9_WINDOW_ID);
        assert_eq!(c.slots.len(), 46);
        assert_eq!(c.slots[CRAFT9_RESULT_SLOT as usize], packets::Slot::empty());
        assert_eq!(c.slots[1], packets::Slot::empty());
        assert_eq!(
            c.slots[10],
            packets::Slot { item_id: mc_world::drops::ITEM_WHEAT, count: 7 },
            "window slot 10 = inventory slot 9"
        );
        assert_eq!(
            c.slots[45],
            packets::Slot { item_id: mc_world::drops::ITEM_STICK, count: 3 },
            "window slot 45 = inventory slot 44 (hotbar)"
        );
        assert_eq!(ds.craft9_session(11).unwrap().x, 4);
        let _ = std::fs::remove_dir_all(ds.save_dir());
    }

    #[test]
    fn craft9_pickup_result_and_quick_move_are_authoritative() {
        let ds = state();
        ds.load_inventory(7, "C9");
        ds.with_inventory(7, |inv| {
            // P60 vanilla：9 小麦必须占 9 格（每格 1 个），单格堆 9 个不出干草捆。
            for i in 0..9 {
                inv.set_slot(9 + i, Some((mc_world::drops::ITEM_WHEAT, 1)));
            }
        });
        ds.open_craft9(7, 0, 64, 0);
        // 背包槽 9+i（窗口 10+i）逐格拿起 → 放入工作台格 1+i（窗口 1+i）。
        let click = |slot, button, ty| packets::ContainerClick {
            container_id: CRAFT9_WINDOW_ID,
            state_id: 0,
            slot,
            button,
            click_type: ty,
        };
        for i in 0..9 {
            assert!(apply_click(&ds, 7, &click(10 + i, 0, packets::CLICK_PICKUP)));
            assert!(apply_click(&ds, 7, &click(1 + i, 0, packets::CLICK_PICKUP)));
        }
        assert!(ds.craft9_session(7).unwrap().grid.iter().all(|s| *s == Some((mc_world::drops::ITEM_WHEAT, 1))));
        // 结果槽取干草捆：9 格各扣 1，全空。
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

    /// P60 拖拽分堆 + 双击收集走服务端三段式/整单（问题 3 的回弹即旧 no-op）。
    #[test]
    fn p60_drag_paint_distributes_and_collect_gathers() {
        let ds = state();
        ds.load_inventory(7, "C9");
        let planks = mc_world::drops::ITEM_OAK_PLANKS;
        ds.with_inventory(7, |inv| {
            inv.set_slot(9, Some((planks, 10)));
            // 收集目标：背包 slot 20 再放 4 个同种。
            inv.set_slot(20, Some((planks, 4)));
        });
        ds.open_craft9(7, 0, 64, 0);
        let click = |slot, button, ty| packets::ContainerClick {
            container_id: CRAFT9_WINDOW_ID,
            state_id: 0,
            slot,
            button,
            click_type: ty,
        };
        // 拿起 10 木板 → 起拖(格1,左键0) → 划过格1/格2(4) → 落子(8)：均分 5/5。
        assert!(apply_click(&ds, 7, &click(10, 0, packets::CLICK_PICKUP)));
        assert!(apply_click(&ds, 7, &click(1, 0, packets::CLICK_QUICK_CRAFT)));
        assert!(apply_click(&ds, 7, &click(1, 4, packets::CLICK_QUICK_CRAFT)));
        assert!(apply_click(&ds, 7, &click(2, 4, packets::CLICK_QUICK_CRAFT)));
        assert!(apply_click(&ds, 7, &click(0, 8, packets::CLICK_QUICK_CRAFT)));
        let grid = ds.craft9_session(7).unwrap().grid;
        assert_eq!(grid[0], Some((planks, 5)));
        assert_eq!(grid[1], Some((planks, 5)));
        assert_eq!(ds.with_inventory(7, |inv| inv.cursor()), Some(None));
        // 右键拖 4 个过 3 格：每格 1 个，余 1。
        ds.with_inventory(7, |inv| inv.set_cursor(Some((planks, 4))));
        assert!(apply_click(&ds, 7, &click(3, 1, packets::CLICK_QUICK_CRAFT)));
        for s in [3, 4, 5] {
            assert!(apply_click(&ds, 7, &click(s, 5, packets::CLICK_QUICK_CRAFT)));
        }
        assert!(apply_click(&ds, 7, &click(0, 9, packets::CLICK_QUICK_CRAFT)));
        let grid = ds.craft9_session(7).unwrap().grid;
        assert_eq!(grid[2], Some((planks, 1)), "right-drag 1 onto empty");
        assert_eq!(grid[3], Some((planks, 1)));
        assert_eq!(grid[4], Some((planks, 1)));
        assert_eq!(ds.with_inventory(7, |inv| inv.cursor()), Some(Some((planks, 1))));
        // 双击收集：光标 1 个 + 格里/背包同种 → 全收到光标。
        assert!(apply_click(&ds, 7, &click(1, 0, packets::CLICK_PICKUP_ALL)));
        let grid = ds.craft9_session(7).unwrap().grid;
        assert!(grid.iter().all(|s| s.is_none()));
        // 1 + 5 + 5 + 1 + 1 + 1 + 背包 4 = 18。
        assert_eq!(
            ds.with_inventory(7, |inv| inv.cursor()),
            Some(Some((planks, 18)))
        );
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

    /// P56 / 26.3 `CraftingMenu.quickMoveStack`：背包 SHIFT 先补 1..=9 合成格
    /// （同物合并、再填空格），剩余留在背包槽并继续 main<->hotbar 搬运。
    #[test]
    fn p56_inventory_shift_fills_grid_before_other_inventory_rows() {
        let ds = state();
        ds.load_inventory(21, "P56Grid");
        ds.with_inventory(21, |inv| {
            inv.set_slot(9, Some((mc_world::drops::ITEM_WHEAT, 9)));
        });
        ds.open_craft9(21, 0, 64, 0);
        let click = |slot, ty| packets::ContainerClick {
            container_id: CRAFT9_WINDOW_ID,
            state_id: 0,
            slot,
            button: 0,
            click_type: ty,
        };
        // 窗口槽 10 = 背包槽 9；SHIFT 把它搬进第一个空合成格。
        assert!(apply_click(&ds, 21, &click(10, packets::CLICK_QUICK_MOVE)));
        assert_eq!(ds.craft9_session(21).unwrap().grid[0], Some((mc_world::drops::ITEM_WHEAT, 9)));
        assert_eq!(ds.with_inventory(21, |inv| inv.get(9)), Some(None));
        // 再放一组同物到背包槽 9，SHIFT 先与格内同物合并（未满 64）。
        ds.with_inventory(21, |inv| {
            inv.set_slot(9, Some((mc_world::drops::ITEM_WHEAT, 5)));
        });
        assert!(apply_click(&ds, 21, &click(10, packets::CLICK_QUICK_MOVE)));
        assert_eq!(ds.craft9_session(21).unwrap().grid[0], Some((mc_world::drops::ITEM_WHEAT, 14)));
        assert_eq!(ds.with_inventory(21, |inv| inv.get(9)), Some(None));
        // 非背包窗口槽（结果/合成格）不会走这条补给路径。
        assert_eq!(inv_slot(0), None);
        assert_eq!(inv_slot(9), None);
        let _ = std::fs::remove_dir_all(ds.save_dir());
    }

    /// P56 / 26.3 `ClientboundContainerSetSlotPacket.STREAM_CODEC`：
    /// CONTAINER_ID(VarInt) + stateId(VarInt) + slot(ShortBE) + ItemStack。
    #[test]
    fn p56_result_slot_packet_body_matches_jar_codec() {
        let ds = state();
        ds.load_inventory(22, "P56Slot");
        let body = build_result_slot_body(&ds, 22).expect("loaded inventory");
        let (packet, used) = packets::SetContainerSlot::decode_body(&body).unwrap();
        assert_eq!(used, body.len(), "no trailing bytes");
        assert_eq!(packet.window_id, CRAFT9_WINDOW_ID);
        assert_eq!(packet.state_id, 2, "load state=1, next_state=2");
        assert_eq!(packet.slot, CRAFT9_RESULT_SLOT);
        assert_eq!(packet.data, packets::Slot::empty());
        // window=1 / state=2 / slot=0x0000 / empty stack=0x00。
        assert_eq!(&body[..4], &[1, 2, 0, 0]);
        let _ = std::fs::remove_dir_all(ds.save_dir());
    }
}
