//! inventory: 服务端权威玩家背包（36 格主+快捷栏口径，46 槽全量同步），只兼容 26.3/777。
//!
//! 槽位（Inventory 窗口表 Player Inventory，与 `container_set_content` 46 槽一致）：
//! 0=合成输出 1-4=合成输入 5-8=盔甲 9-35=主背包 36-44=快捷栏 45=副手。
//! P18 只用 9-35 与 36-44（拾取填充），`selected` 0..=8 对应 36+selected（C2S 0x36）。
//! 全量同步发 `SetContainerContent`（46 槽），拾取/放置发 `SetContainerSlot`（单槽）。
//!
//! 存档：`player-<user>-<seed>.dat`（seed 隔离），与 overrides 存档同目录。
//! 键用稳定用户名而非 uuid：离线模式 `offline_uuid` 每次登录随机 v4（见
//! `mc_protocol::packets::login::offline_uuid`），uuid 作键则重连永远 miss；
//! 用户名在离线模式是稳定身份。在线模式落地后应切回 uuid 键（TODO）。
//!
//! P40：加入生存三字段（health/food/saturation）并设存档 v3（= v2 + 12B 尾部生存字段），
//! v1/v2 旧档仍可读、生存值取默认满状态；redb `players` 表对应 v2 记录（见 store.rs）。
//! P53：v4 = v3 + 尾部 6B（exhaustion f32 BE + gamemode u8 + tool_enchant u8）；
//! v1–v3 旧档兼容读，缺省 0 / survival / none；redb `players` v3 同口径。
//! P40 合成：槽 1–4 现在允许 PICKUP 放入/取出原料，槽 0 是只读结果槽（点击走
//! `craft_result`）；9–45 的 PICKUP/SWAP/QUICK_MOVE/THROW 语义一字未改。

use std::io::{Read, Write};
use std::path::Path;

use crate::items::MAX_STACK;

/// 全量同步槽位数（container_set_content 固定 46）。
pub const SLOT_COUNT: usize = 46;
/// 主背包槽范围。
pub const MAIN_START: usize = 9;
pub const MAIN_END: usize = 35;
/// 快捷栏槽范围。
pub const HOTBAR_START: usize = 36;
pub const HOTBAR_END: usize = 44;

/// 存档魔数 + 版本（自有格式，与 P17 overrides 存档同目录、seed 隔离）。
/// v2 = v1 + 尾部 4 字节总经验（P31）；v3 = v2 + 尾部 12 字节生存状态（P40：
/// health f32 BE + food i32 BE + saturation f32 BE）；v4 = v3 + 尾部 6 字节
/// （P53：exhaustion f32 BE + gamemode u8 + tool_enchant u8）。
/// v1–v3 旧档仍可读，XP/生存/疲劳/设置分别缺省 0 / 满状态 / 0 / survival,none。
const SAVE_MAGIC: [u8; 4] = *b"P18I";
const SAVE_VERSION: u8 = 4;
const LEGACY_SAVE_VERSION: u8 = 1;
const LEGACY_SAVE_VERSION_V2: u8 = 2;
const LEGACY_SAVE_VERSION_V3: u8 = 3;

/// P53 A4：游戏模式存档码（与 `mc_network::GameMode` 的 as_str 顺序无关，固定映射）。
pub const SETTINGS_GAMEMODE_SURVIVAL: u8 = 0;
pub const SETTINGS_GAMEMODE_CREATIVE: u8 = 1;
/// P53 A4：工具附魔存档码（none / fortune / silk_touch）。
pub const SETTINGS_ENCHANT_NONE: u8 = 0;
pub const SETTINGS_ENCHANT_FORTUNE: u8 = 1;
pub const SETTINGS_ENCHANT_SILK_TOUCH: u8 = 2;
/// 新背包开局石头数（hotbar 槽 36）。
pub const STARTER_COUNT: i32 = 64;

/// 服务端权威玩家背包（单玩家实例；多连接按 conn_id 各持一份，见 net 层 DropState）。
#[derive(Debug, Clone)]
pub struct PlayerInventory {
    slots: [Option<(i32, i32)>; SLOT_COUNT],
    selected: u8,
    state_id: i32,
    /// 鼠标手持栈（ContainerClick 拖拽中转；不存盘，断线时 `stow_cursor` 收回）。
    cursor: Option<(i32, i32)>,
    /// P31 总经验值（与背包同文件存盘；旧存档缺省 0）。
    total_xp: i32,
    /// P40 生存状态（与背包同文件/同 redb 记录存盘；旧档默认满血满饥饿）。
    survival: crate::survival::SurvivalState,
    /// P53 A2：疲劳度浮点伴随状态（旧档缺字段按 0 处理）。
    exhaustion: f32,
    /// P53 A4：跨重连持久化的后台设置（旧档默认 survival / none）。
    gamemode: u8,
    tool_enchant: u8,
}

impl Default for PlayerInventory {
    fn default() -> Self {
        Self {
            slots: [None; SLOT_COUNT],
            selected: 0,
            state_id: 1,
            cursor: None,
            total_xp: 0,
            survival: crate::survival::SurvivalState::full(),
            exhaustion: 0.0,
            gamemode: SETTINGS_GAMEMODE_SURVIVAL,
            tool_enchant: SETTINGS_ENCHANT_NONE,
        }
    }
}

impl PlayerInventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// 鼠标手持栈（同步给客户端 carried 用）。
    pub fn cursor(&self) -> Option<(i32, i32)> {
        self.cursor
    }

    /// P47 工作台会话用：直接设置光标栈（背包 2×2 的 click 语义不变）。
    pub fn set_cursor(&mut self, stack: Option<(i32, i32)>) {
        self.cursor = stack;
    }

    /// 手持栈搬回背包（断线时调用，避免凭空消失；实在装不下才丢弃）。
    pub fn stow_cursor(&mut self) {
        while let Some((id, c)) = self.cursor {
            let left = self.add_item(id, c);
            if left >= c {
                self.cursor = None;
                break;
            }
            self.cursor = if left > 0 { Some((id, left)) } else { None };
        }
    }

    /// 集装箱点击应用（玩家背包窗口；返回 true=状态变了）：
    /// P40 起 PICKUP 额外接受合成格 1..=4；SWAP/QUICK_MOVE/THROW 仍只处理 9..=45。
    /// - PICKUP(0) 左键 button=0：拿起整栈 / 放下 / 同物合并 / 异物交换；
    ///   右键 button=1：放一 / 拿半（向上取整）。
    /// - SWAP(2)：button=快捷栏序号 0..=8，与点击槽交换。
    /// - QUICK_MOVE(1)：整栈搬到另一分区（主包<->快捷栏），先合并后进空槽。
    /// 其它类型/越界槽返回 false（调用方全量同步，把客户端预测回滚）。
    pub fn apply_click(&mut self, slot: i16, button: i8, click_type: u8) -> bool {
        match click_type {
            0 => self.click_pickup(slot, button),
            1 => self.click_quick_move(slot),
            2 => self.click_swap_hotbar(slot, button),
            _ => false,
        }
    }

    /// P40：点击合成结果槽（0）= 权威配方判定 + 扣 1-4 格原料 + 结果给光标（同物已满则
    /// 溢出进背包）。两种鼠标键都取结果（原版左/右点击结果槽都是取走整份）。
    /// 不匹配时返回 None，调用方保持结果槽为空、不扣任何原料。
    pub fn craft_result(&mut self, button: i8) -> Option<(i32, i32)> {
        if button != 0 && button != 1 {
            return None;
        }
        let mut grid = [None; crate::crafting::GRID_SIZE];
        for (i, slot) in grid.iter_mut().enumerate() {
            *slot = self.slots[1 + i];
        }
        let recipe = crate::crafting::match_recipe(grid)?;
        let (id, count) = recipe.result;
        // 光标为空直接拿；同物未满则合并、装不下的部分进背包；异物则拒绝（不吞光标物品）。
        match self.cursor {
            None => self.cursor = Some((id, count)),
            Some((cid, cc)) if cid == id => {
                let take = (crate::items::MAX_STACK - cc).min(count);
                self.cursor = Some((id, cc + take));
                let left = count - take;
                if left > 0 {
                    self.add_item(id, left);
                }
            }
            _ => return None,
        }
        // 原料在校验通过后才扣：consume_grid 只会取对应 ID 的数量。
        if crate::crafting::consume_grid(&mut grid, recipe) {
            for (i, slot) in grid.into_iter().enumerate() {
                self.slots[1 + i] = slot;
            }
        }
        self.slots[crate::crafting::RESULT_SLOT] = None;
        Some((id, count))
    }

    /// P56 / 26.3 jar `InventoryMenu.quickMoveStack(player, 0)` +
    /// `AbstractContainerMenu.doClick` QUICK_MOVE 循环：SHIFT 点结果槽时把已算出的结果
    /// 整份搬进背包（jar: `moveItemStackTo(stack, 9, 45, true)`），`ResultSlot.onTake`
    /// 逐格扣原料，客户端再重算结果并重复搬运。这里按同一口径循环“匹配→扣料→入包”，
    /// 但与工作台窗口 1 不同：**背包 2×2 不主动从背包补料**（jar 的 SHIFT 只搬结果，
    /// 没有把背包物品塞回合成格的逻辑）；放不下整份结果时直接停止，绝不吞原料。
    /// 返回本次连续合成次数。
    pub fn craft_result_quick_move(&mut self) -> usize {
        let mut crafted = 0;
        loop {
            let mut grid = [None; crate::crafting::GRID_SIZE];
            for (i, slot) in grid.iter_mut().enumerate() {
                *slot = self.slots[1 + i];
            }
            let Some(recipe) = crate::crafting::match_recipe(grid) else { break };
            let (id, count) = recipe.result;
            if self.free_slots_for(id, count) < count {
                break;
            }
            if !crate::crafting::consume_grid(&mut grid, recipe) {
                break;
            }
            for (i, slot) in grid.into_iter().enumerate() {
                self.slots[1 + i] = slot;
            }
            assert_eq!(self.add_item(id, count), 0, "capacity pre-checked");
            self.slots[crate::crafting::RESULT_SLOT] = None;
            crafted += 1;
        }
        crafted
    }

    /// P56 / 26.3 jar `InventoryMenu.slotsChanged` → `CraftingMenu.slotChangedCraftingGrid`：
    /// 结果槽是服务端按合成格算出来的（`resultSlots.setItem(0, result)`），不是手放进去的。
    /// 这里给出要写进全量/单槽同步包槽 0 的协议 Slot；没有配方时为空。
    pub fn craft_result_protocol_slot(&self) -> mc_protocol::packets::Slot {
        let mut grid = [None; crate::crafting::GRID_SIZE];
        for (i, slot) in grid.iter_mut().enumerate() {
            *slot = self.slots[1 + i];
        }
        match crate::crafting::match_recipe(grid) {
            Some(recipe) => mc_protocol::packets::Slot {
                item_id: recipe.result.0,
                count: recipe.result.1,
            },
            None => mc_protocol::packets::Slot::empty(),
        }
    }

    /// P56 / 26.3 jar `AbstractContainerMenu.doClick` SWAP 分支作用于 `ResultSlot`：
    /// button 0..=8 = 快捷栏槽 36..44，button 40 = 副手槽 45。jar 里结果槽
    /// `mayPlace()==false`，所以目标槽非空时 jar 直接 no-op；本服另外要求光标为空
    /// （避免重复产物）。成功时权威合成结果直接落进该槽，原槽清空；越界一律无操作。
    pub fn craft_result_swap(&mut self, button: i8) -> bool {
        if !(0..=8).contains(&button) && button != 40 {
            return false;
        }
        let idx = if button == 40 { 45 } else { HOTBAR_START + button as usize };
        if self.cursor.is_some() || self.slots[idx].is_some() {
            return false;
        }
        let Some((id, count)) = self.craft_result(0) else { return false };
        self.slots[idx] = Some((id, count));
        self.cursor = None;
        true
    }

    /// P56 / 26.3 jar `InventoryMenu.quickMoveStack` 对合成格 1..=4 的 SHIFT：
    /// `moveItemStackTo(stack, 9, 45, false)` —— 先同物合并，再填入空格（主包+快捷栏，
    /// 不含副手 45）。旧实现的 `click_quick_move` 只认 9..=45，格内 SHIFT 是 no-op。
    pub fn quick_move_grid_slot(&mut self, slot: i16) -> bool {
        if !(1..=4).contains(&slot) {
            return false;
        }
        let idx = slot as usize;
        let Some((id, original)) = self.slots[idx] else { return false };
        let mut count = original;
        for t in (MAIN_START..=MAIN_END).chain(HOTBAR_START..=HOTBAR_END) {
            if count <= 0 {
                break;
            }
            if let Some((tid, tc)) = self.slots[t] {
                if tid == id && tc < MAX_STACK {
                    let take = (MAX_STACK - tc).min(count);
                    self.slots[t] = Some((id, tc + take));
                    count -= take;
                }
            }
        }
        for t in (HOTBAR_START..=HOTBAR_END).chain(MAIN_START..=MAIN_END) {
            if count <= 0 {
                break;
            }
            if self.slots[t].is_none() {
                let take = MAX_STACK.min(count);
                self.slots[t] = Some((id, take));
                count -= take;
            }
        }
        self.slots[idx] = if count > 0 { Some((id, count)) } else { None };
        count < original
    }

    /// 可操作槽（P18 原有语义，保持不变）：主背包 9-35、快捷栏 36-44、副手 45；
    /// 合成/盔甲 0-8 不碰。SWAP / QUICK_MOVE / THROW 继续走这里。
    fn play_slot(&self, slot: i16) -> Option<usize> {
        if (9..=45).contains(&slot) {
            Some(slot as usize)
        } else {
            None
        }
    }

    /// P40 合成格：1-4 也允许 PICKUP（放/取原料）；槽 0 结果由 `craft_result` 处理。
    /// 只扩展 PICKUP，不改 9-45 原有语义，也不碰盔甲 5-8。
    fn pickup_slot(&self, slot: i16) -> Option<usize> {
        if (1..=4).contains(&slot) || (9..=45).contains(&slot) {
            Some(slot as usize)
        } else {
            None
        }
    }

    fn click_pickup(&mut self, slot: i16, button: i8) -> bool {
        let idx = match self.pickup_slot(slot) {
            Some(i) => i,
            None => return false,
        };
        if button == 0 {
            match (self.slots[idx], self.cursor) {
                (None, None) => false,
                (Some(stack), None) => {
                    self.cursor = Some(stack);
                    self.slots[idx] = None;
                    true
                }
                (None, Some(c)) => {
                    self.slots[idx] = Some(c);
                    self.cursor = None;
                    true
                }
                (Some((aid, ac)), Some((bid, bc))) if aid == bid => {
                    let room = MAX_STACK - ac;
                    if room <= 0 {
                        return false;
                    }
                    let take = room.min(bc);
                    self.slots[idx] = Some((aid, ac + take));
                    self.cursor = if bc > take { Some((bid, bc - take)) } else { None };
                    true
                }
                (Some(a), Some(b)) => {
                    self.slots[idx] = Some(b);
                    self.cursor = Some(a);
                    true
                }
            }
        } else if button == 1 {
            match (self.slots[idx], self.cursor) {
                (None, None) => false,
                (None, Some((bid, bc))) => {
                    self.slots[idx] = Some((bid, 1));
                    self.cursor = if bc > 1 { Some((bid, bc - 1)) } else { None };
                    true
                }
                (Some((aid, ac)), None) => {
                    let take = (ac + 1) / 2;
                    self.cursor = Some((aid, take));
                    self.slots[idx] = if ac > take { Some((aid, ac - take)) } else { None };
                    true
                }
                (Some((aid, ac)), Some((bid, bc))) if aid == bid && ac < MAX_STACK => {
                    self.slots[idx] = Some((aid, ac + 1));
                    self.cursor = if bc > 1 { Some((bid, bc - 1)) } else { None };
                    true
                }
                _ => false,
            }
        } else {
            false
        }
    }

    fn click_swap_hotbar(&mut self, slot: i16, button: i8) -> bool {
        let idx = match self.play_slot(slot) {
            Some(i) => i,
            None => return false,
        };
        if !(0..=8).contains(&button) {
            return false;
        }
        let hidx = HOTBAR_START + button as usize;
        if idx == hidx {
            return false;
        }
        self.slots.swap(idx, hidx);
        true
    }

    fn click_quick_move(&mut self, slot: i16) -> bool {
        let idx = match self.play_slot(slot) {
            Some(i) => i,
            None => return false,
        };
        let (id, mut count) = match self.slots[idx] {
            Some(s) => s,
            None => return false,
        };
        // 目标分区：主包 <-> 快捷栏（副手 45 视为快捷栏侧，搬进主包）。
        let targets: Vec<usize> = if (MAIN_START..=MAIN_END).contains(&idx) {
            (HOTBAR_START..=HOTBAR_END).collect()
        } else {
            (MAIN_START..=MAIN_END).collect()
        };
        let mut moved = false;
        for t in targets {
            if count <= 0 {
                break;
            }
            match self.slots[t] {
                Some((tid, tc)) if tid == id && tc < MAX_STACK => {
                    let take = (MAX_STACK - tc).min(count);
                    self.slots[t] = Some((tid, tc + take));
                    count -= take;
                    moved = true;
                }
                None => {
                    let take = MAX_STACK.min(count);
                    self.slots[t] = Some((id, take));
                    count -= take;
                    moved = true;
                }
                _ => {}
            }
        }
        if !moved {
            return false;
        }
        self.slots[idx] = if count > 0 { Some((id, count)) } else { None };
        true
    }

    pub fn selected(&self) -> u8 {
        self.selected
    }

    pub fn set_selected(&mut self, v: u8) {
        if v <= 8 {
            self.selected = v;
        }
    }

    pub fn selected_slot_index(&self) -> usize {
        HOTBAR_START + self.selected as usize
    }

    pub fn state_id(&self) -> i32 {
        self.state_id
    }

    /// P56 / 26.3 jar `AbstractContainerMenu.incrementStateId()`:
    /// `stateId = stateId + 1 & Short.MAX_VALUE`（15 位回绕：0x7fff → 0）。
    /// 正常会话的 state id 远小于 32768，既有包流逐字节不变；只在长会话回绕时生效。
    pub fn next_state(&mut self) -> i32 {
        self.state_id = (self.state_id + 1) & 0x7fff;
        self.state_id
    }

    // ---- P31 经验（与背包同存档；SetExperience 三字段由 xp 模块换算） ----

    /// 当前总经验值。
    pub fn total_xp(&self) -> i32 {
        self.total_xp
    }

    /// 拾取经验球后累加（返回新的总值；溢出钳制到 i32::MAX）。
    pub fn add_experience(&mut self, amount: i32) -> i32 {
        self.total_xp = (self.total_xp as i64 + amount.max(0) as i64).clamp(0, i32::MAX as i64) as i32;
        self.total_xp
    }

    /// 直接覆盖总经验（读盘/测试用；负值归零）。
    pub fn set_total_xp(&mut self, total: i32) {
        self.total_xp = total.max(0);
    }

    /// 当前经验等级。
    pub fn xp_level(&self) -> i32 {
        crate::xp::level_for_xp(self.total_xp)
    }

    /// 当前等级的经验条进度 0.0..=1.0。
    pub fn xp_progress(&self) -> f32 {
        crate::xp::xp_progress(self.total_xp)
    }

    // ---- P40 生存（与背包同存档/同 redb 记录） ----

    /// 当前生存状态（生命/饥饿/饱和）的只读快照。
    pub fn survival(&self) -> crate::survival::SurvivalState {
        self.survival
    }

    /// 覆盖生存状态（读盘/复活/测试）。保存前会钳制到合法范围。
    pub fn set_survival(&mut self, s: crate::survival::SurvivalState) {
        self.survival = s.sanitize();
    }

    /// 对生存状态做原地修改（ticker/进食/伤害入口）。
    pub fn with_survival<R>(&mut self, f: impl FnOnce(&mut crate::survival::SurvivalState) -> R) -> R {
        f(&mut self.survival)
    }

    // ---- P53：疲劳度 + 跨重连设置（与背包同存档） ----

    pub fn exhaustion(&self) -> f32 {
        self.exhaustion
    }

    pub fn set_exhaustion(&mut self, value: f32) {
        self.exhaustion = crate::survival::sanitize_exhaustion(value);
    }

    /// P53 A4：模式存档码（0=survival，1=creative；非法值归零）。
    pub fn persisted_gamemode(&self) -> u8 {
        if self.gamemode == SETTINGS_GAMEMODE_CREATIVE {
            SETTINGS_GAMEMODE_CREATIVE
        } else {
            SETTINGS_GAMEMODE_SURVIVAL
        }
    }

    pub fn set_persisted_gamemode(&mut self, code: u8) {
        self.gamemode = if code == SETTINGS_GAMEMODE_CREATIVE {
            SETTINGS_GAMEMODE_CREATIVE
        } else {
            SETTINGS_GAMEMODE_SURVIVAL
        };
    }

    /// P53 A4：附魔存档码（0=none，1=fortune，2=silk_touch；非法值归零）。
    pub fn persisted_tool_enchant(&self) -> u8 {
        match self.tool_enchant {
            SETTINGS_ENCHANT_FORTUNE => SETTINGS_ENCHANT_FORTUNE,
            SETTINGS_ENCHANT_SILK_TOUCH => SETTINGS_ENCHANT_SILK_TOUCH,
            _ => SETTINGS_ENCHANT_NONE,
        }
    }

    pub fn set_persisted_tool_enchant(&mut self, code: u8) {
        self.tool_enchant = match code {
            SETTINGS_ENCHANT_FORTUNE => SETTINGS_ENCHANT_FORTUNE,
            SETTINGS_ENCHANT_SILK_TOUCH => SETTINGS_ENCHANT_SILK_TOUCH,
            _ => SETTINGS_ENCHANT_NONE,
        };
    }

    /// 清空所有槽（死亡掉落用；光标也收回后丢弃）。
    pub fn clear_all(&mut self) {
        for slot in self.slots.iter_mut() {
            *slot = None;
        }
        self.cursor = None;
    }

    pub fn get(&self, idx: usize) -> Option<(i32, i32)> {
        self.slots.get(idx).copied().flatten()
    }

    /// 直接写入指定槽（P36 存储原型的读盘/迁移辅助；不校验堆叠上限，
    /// 调用方负责合法性）。生产逻辑仍走 `add_item`/`apply_click`。
    pub fn set_slot(&mut self, idx: usize, stack: Option<(i32, i32)>) -> bool {
        if idx >= SLOT_COUNT {
            return false;
        }
        self.slots[idx] = stack;
        true
    }

    /// 读盘恢复 state_id（P36 存储原型用；钳制到 >=1 与旧读盘口径一致）。
    pub fn set_state_id(&mut self, v: i32) {
        self.state_id = v.max(1);
    }

    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }

    /// 非空槽数（诊断/测试用）。
    pub fn filled_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 新背包开局：hotbar 槽 36 给 64 石头（保证“放置扣 1”可验证；生存模式替换）。
    pub fn slots_hotbar_starter(&mut self) {
        self.slots[HOTBAR_START] = Some((crate::drops::ITEM_STONE, STARTER_COUNT));
    }

    /// 手持槽物品（selected 对应快捷栏槽）。
    pub fn held(&self) -> Option<(i32, i32)> {
        self.get(self.selected_slot_index())
    }

    /// 拾取入库：先填同物品未满栈（只扫主背包 9-35 与快捷栏 36-44，不碰合成/盔甲槽），
    /// 再找空槽（**快捷栏 36-44 优先**，与原版一致；主背包 9-35 其次；副手 45 最后）；
    /// 返回未装下的剩余数量（0=全装下）。
    /// P47：不修改背包，预估还能放下多少个 `item_id`（SHIFT 连续合成前的能力检查，
    /// 保证扣了合成原料后不会吞掉放不下的产物）。
    pub fn free_slots_for(&self, item_id: i32, limit: i32) -> i32 {
        let mut room = 0;
        for idx in (MAIN_START..=MAIN_END).chain(HOTBAR_START..=HOTBAR_END) {
            match self.slots[idx] {
                Some((id, count)) if id == item_id => room += MAX_STACK - count,
                None => room += MAX_STACK,
                _ => {}
            }
            if room >= limit {
                return room;
            }
        }
        // add_item 的空槽兜底也包含副手 45（但副手不参与同物合并）。
        if self.slots[45].is_none() {
            room += MAX_STACK;
        }
        room
    }

    pub fn add_item(&mut self, item_id: i32, mut count: i32) -> i32 {
        for idx in (MAIN_START..=MAIN_END).chain(HOTBAR_START..=HOTBAR_END) {
            if count <= 0 {
                break;
            }
            let merge = match self.slots[idx] {
                Some((id, c)) if id == item_id && c < MAX_STACK => Some((id, c)),
                _ => None,
            };
            if let Some((id, c)) = merge {
                let room = MAX_STACK - c;
                let take = room.min(count);
                self.slots[idx] = Some((id, c + take));
                count -= take;
            }
        }
        for idx in
            (HOTBAR_START..=HOTBAR_END).chain(MAIN_START..=MAIN_END).chain([45].into_iter())
        {
            if count <= 0 {
                break;
            }
            if self.slots[idx].is_none() {
                let take = MAX_STACK.min(count);
                self.slots[idx] = Some((item_id, take));
                count -= take;
            }
        }
        count
    }

    /// 手持槽扣 1（放置成功用）；返回 (item_id, 剩余count)；空手返回 None。
    pub fn consume_held(&mut self) -> Option<(i32, i32)> {
        let idx = self.selected_slot_index();
        let (id, c) = self.slots[idx]?;
        if c <= 1 {
            self.slots[idx] = None;
            Some((id, 0))
        } else {
            self.slots[idx] = Some((id, c - 1));
            Some((id, c - 1))
        }
    }

    /// Q 扔手持（P19；原版 `ServerboundPlayerActionPacket$Action` DROP 语义）：
    /// `all=false`（DROP_ITEM=5，Q）扔 1 个，`all=true`（DROP_ALL_ITEMS=4，Ctrl+Q）
    /// 扔整栈；返回 (item_id, 扔出数量)。空手返回 None（调用方只 ack，不落子不断线）。
    pub fn throw_held(&mut self, all: bool) -> Option<(i32, i32)> {
        let idx = self.selected_slot_index();
        let (id, c) = self.slots[idx]?;
        if all || c <= 1 {
            self.slots[idx] = None;
            Some((id, c))
        } else {
            self.slots[idx] = Some((id, c - 1));
            Some((id, 1))
        }
    }

    /// 背包打开时按 Q（P19；原版 `AbstractContainerMenu.doClick` 的 THROW 分支）：
    /// 要求光标为空（手持栈时原版无操作，返回 None）；button=0 扔 1 个，
    /// button=1 扔整栈；其它 button/越界槽/空槽返回 None。
    /// 返回 (item_id, 扔出数量)，调用方在脚下生成掉落实体。
    pub fn throw_from_slot(&mut self, slot: i16, button: i8) -> Option<(i32, i32)> {
        if self.cursor.is_some() {
            return None;
        }
        let idx = self.play_slot(slot)?;
        let (id, c) = self.slots[idx]?;
        match button {
            0 => {
                if c <= 1 {
                    self.slots[idx] = None;
                    Some((id, 1))
                } else {
                    self.slots[idx] = Some((id, c - 1));
                    Some((id, 1))
                }
            }
            1 => {
                self.slots[idx] = None;
                Some((id, c))
            }
            _ => None,
        }
    }

    /// 全量同步槽（46 个 Slot，空位 count=0）。
    pub fn to_protocol_slots(&self) -> Vec<mc_protocol::packets::Slot> {
        self.slots
            .iter()
            .map(|s| match s {
                Some((id, c)) => mc_protocol::packets::Slot { count: *c, item_id: *id },
                None => mc_protocol::packets::Slot::empty(),
            })
            .collect()
    }

    pub fn protocol_slot(&self, idx: usize) -> mc_protocol::packets::Slot {
        match self.get(idx) {
            Some((id, c)) => mc_protocol::packets::Slot { count: c, item_id: id },
            None => mc_protocol::packets::Slot::empty(),
        }
    }

    /// 鼠标手持同步槽（Content carried 用）。
    pub fn cursor_slot(&self) -> mc_protocol::packets::Slot {
        match self.cursor {
            Some((id, c)) => mc_protocol::packets::Slot { count: c, item_id: id },
            None => mc_protocol::packets::Slot::empty(),
        }
    }

    /// 存档路径：`dir/player-<user>-<seed>.dat`（user 为稳定用户名，见模块文档）。
    pub fn save_path(dir: &Path, user_tag: &str, seed: i64) -> std::path::PathBuf {
        dir.join(format!("player-{user_tag}-{seed}.dat"))
    }

    /// 落盘（tmp+rename 原子替换）：魔数/版本/selected/state/条目(槽u8+物品i32+数量i32)
    /// + 尾部 4B 总经验（v2）+ 12B 生存状态（v3）+ 6B 疲劳/设置（v4，P53）。
    pub fn save_to_dir(&self, dir: &Path, user_tag: &str, seed: i64) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut entries = Vec::new();
        for (i, s) in self.slots.iter().enumerate() {
            if let Some((id, c)) = s {
                entries.push((i as u8, *id, *c));
            }
        }
        let path = Self::save_path(dir, user_tag, seed);
        let tmp = path.with_extension("tmp");
        let mut buf = Vec::with_capacity(12 + entries.len() * 9 + 4 + 12 + 6);
        buf.extend_from_slice(&SAVE_MAGIC);
        buf.push(SAVE_VERSION);
        buf.push(self.selected);
        buf.extend_from_slice(&self.state_id.to_be_bytes());
        buf.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        for (slot, id, count) in &entries {
            buf.push(*slot);
            buf.extend_from_slice(&id.to_be_bytes());
            buf.extend_from_slice(&count.to_be_bytes());
        }
        // v2：尾部追加总经验（i32 BE）；旧版本文件无此字段，读盘按 0 处理。
        buf.extend_from_slice(&self.total_xp.to_be_bytes());
        // v3：生存三字段（f32/i32/f32 BE），旧版本文件无此字段，读盘按满状态处理。
        buf.extend_from_slice(&self.survival.health.to_be_bytes());
        buf.extend_from_slice(&self.survival.food.to_be_bytes());
        buf.extend_from_slice(&self.survival.saturation.to_be_bytes());
        // v4（P53）：exhaustion f32 BE + gamemode u8 + tool_enchant u8。
        buf.extend_from_slice(&self.exhaustion.to_be_bytes());
        buf.push(self.gamemode);
        buf.push(self.tool_enchant);
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&buf)?;
            f.flush()?;
        }
        std::fs::rename(&tmp, &path)?;
        Ok(entries.len())
    }

    /// 读盘（缺文件=空背包，返回 (inventory, is_new)；损坏报错由调用方回退空背包）。
    pub fn load_from_dir(dir: &Path, user_tag: &str, seed: i64) -> std::io::Result<(Self, bool)> {
        let path = Self::save_path(dir, user_tag, seed);
        let mut f = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Self::new(), true)),
            Err(e) => return Err(e),
        };
        let mut head = [0u8; 12];
        f.read_exact(&mut head).map_err(|e| {
            std::io::Error::new(e.kind(), format!("{}: 头损坏 ({e})", path.display()))
        })?;
        if head[0..4] != SAVE_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 魔数不符（非 P18 背包存档）", path.display()),
            ));
        }
        if head[4] != SAVE_VERSION
            && head[4] != LEGACY_SAVE_VERSION
            && head[4] != LEGACY_SAVE_VERSION_V2
            && head[4] != LEGACY_SAVE_VERSION_V3
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 版本 {} 不支持", path.display(), head[4]),
            ));
        }
        let mut inv = Self::new();
        inv.selected = if head[5] <= 8 { head[5] } else { 0 };
        inv.state_id = i32::from_be_bytes(head[6..10].try_into().unwrap()).max(1);
        let count = u16::from_be_bytes(head[10..12].try_into().unwrap()) as usize;
        if count > SLOT_COUNT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 条目数 {count} 过大", path.display()),
            ));
        }
        for _ in 0..count {
            let mut e = [0u8; 9];
            f.read_exact(&mut e).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 条目截断 ({e})", path.display()))
            })?;
            let slot = e[0] as usize;
            let id = i32::from_be_bytes(e[1..5].try_into().unwrap());
            let c = i32::from_be_bytes(e[5..9].try_into().unwrap());
            if slot < SLOT_COUNT && id > 0 && c > 0 && c <= MAX_STACK {
                inv.slots[slot] = Some((id, c));
            }
        }
        // v2+ 尾部 4 字节总经验；v1 无此字段，XP 缺省 0（向后兼容旧存档）。
        if head[4] == SAVE_VERSION || head[4] == LEGACY_SAVE_VERSION_V2 || head[4] == LEGACY_SAVE_VERSION_V3 {
            let mut xp = [0u8; 4];
            f.read_exact(&mut xp).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 经验字段截断 ({e})", path.display()))
            })?;
            inv.total_xp = i32::from_be_bytes(xp).max(0);
        }
        // v3+ 生存三字段；v1/v2 无此字段，生存状态取默认满值（向后兼容）。
        if head[4] == SAVE_VERSION || head[4] == LEGACY_SAVE_VERSION_V3 {
            let mut health = [0u8; 4];
            let mut food = [0u8; 4];
            let mut sat = [0u8; 4];
            f.read_exact(&mut health).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 生命字段截断 ({e})", path.display()))
            })?;
            f.read_exact(&mut food).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 饥饿字段截断 ({e})", path.display()))
            })?;
            f.read_exact(&mut sat).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 饱和字段截断 ({e})", path.display()))
            })?;
            inv.set_survival(crate::survival::SurvivalState {
                health: f32::from_be_bytes(health),
                food: i32::from_be_bytes(food),
                saturation: f32::from_be_bytes(sat),
            });
        }
        // v4+ 疲劳/设置；v1–v3 旧档缺字段按 0 / survival / none（向后兼容）。
        if head[4] == SAVE_VERSION {
            let mut ex = [0u8; 4];
            f.read_exact(&mut ex).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 疲劳字段截断 ({e})", path.display()))
            })?;
            let mut flags = [0u8; 2];
            f.read_exact(&mut flags).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 设置字段截断 ({e})", path.display()))
            })?;
            inv.set_exhaustion(f32::from_be_bytes(ex));
            inv.set_persisted_gamemode(flags[0]);
            inv.set_persisted_tool_enchant(flags[1]);
        }
        let mut tail = [0u8; 1];
        if f.read(&mut tail)? != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 文件尾有多余字节", path.display()),
            ));
        }
        Ok((inv, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickup_merges_and_overflows_to_empty() {
        let mut inv = PlayerInventory::new();
        // 空槽顺序快捷栏优先：先落 36。
        assert_eq!(inv.add_item(55, 3), 0);
        assert_eq!(inv.get(36), Some((55, 3)));
        // 同物品继续合并到第一栈。
        assert_eq!(inv.add_item(55, 61), 0);
        assert_eq!(inv.get(36), Some((55, 64)));
        // 64 满后溢出到下一快捷栏槽。
        assert_eq!(inv.add_item(55, 2), 0);
        assert_eq!(inv.get(37), Some((55, 2)));
        // 背包同步 46 槽。
        assert_eq!(inv.to_protocol_slots().len(), SLOT_COUNT);
    }

    #[test]
    fn consume_held_decrements_and_clears() {
        let mut inv = PlayerInventory::new();
        inv.set_selected(0);
        assert_eq!(inv.consume_held(), None);
        inv.add_item(1, 2);
        // add 先填快捷栏 36，手持 36 为空 → 仍 None；手动选中验证用快捷栏槽。
        inv.slots[36] = Some((1, 2));
        assert_eq!(inv.consume_held(), Some((1, 1)));
        assert_eq!(inv.get(36), Some((1, 1)));
        assert_eq!(inv.consume_held(), Some((1, 0)));
        assert_eq!(inv.get(36), None);
        assert_eq!(inv.consume_held(), None);
    }

    #[test]
    fn throw_held_single_all_and_empty() {
        let mut inv = PlayerInventory::new();
        inv.set_selected(0);
        assert_eq!(inv.throw_held(false), None, "empty hand throws nothing");
        assert_eq!(inv.throw_held(true), None);
        inv.slots[36] = Some((1, 3));
        // Q：扔 1 个，槽剩 2。
        assert_eq!(inv.throw_held(false), Some((1, 1)));
        assert_eq!(inv.get(36), Some((1, 2)));
        // Ctrl+Q：整组扔完，槽变空。
        assert_eq!(inv.throw_held(true), Some((1, 2)));
        assert_eq!(inv.get(36), None);
        // 单个整组扔：一次清空。
        inv.slots[36] = Some((55, 1));
        assert_eq!(inv.throw_held(true), Some((55, 1)));
        assert_eq!(inv.get(36), None);
        inv.slots[36] = Some((55, 1));
        assert_eq!(inv.throw_held(false), Some((55, 1)));
        assert_eq!(inv.get(36), None);
    }

    #[test]
    fn throw_from_slot_button_cursor_and_bounds() {
        let mut inv = PlayerInventory::new();
        inv.slots[9] = Some((55, 10));
        // button=0 扔 1 个。
        assert_eq!(inv.throw_from_slot(9, 0), Some((55, 1)));
        assert_eq!(inv.get(9), Some((55, 9)));
        // button=1 扔整栈。
        assert_eq!(inv.throw_from_slot(9, 1), Some((55, 9)));
        assert_eq!(inv.get(9), None);
        // 空槽/越界/非法 button 无操作。
        assert_eq!(inv.throw_from_slot(9, 0), None);
        assert_eq!(inv.throw_from_slot(0, 0), None, "crafting slots untouched");
        assert_eq!(inv.throw_from_slot(46, 0), None);
        assert_eq!(inv.throw_from_slot(-999, 0), None);
        inv.slots[10] = Some((1, 5));
        assert_eq!(inv.throw_from_slot(10, 2), None);
        assert_eq!(inv.get(10), Some((1, 5)));
        // 光标上有东西时按 Q：原版无操作（doClick THROW 要求 carried 为空）。
        inv.cursor = Some((62, 3));
        assert_eq!(inv.throw_from_slot(10, 0), None);
        assert_eq!(inv.throw_from_slot(10, 1), None);
        assert_eq!(inv.get(10), Some((1, 5)));
        assert_eq!(inv.cursor(), Some((62, 3)));
    }

    #[test]
    fn selected_maps_to_hotbar_and_clamps() {
        let mut inv = PlayerInventory::new();
        inv.set_selected(3);
        assert_eq!(inv.selected_slot_index(), 39);
        inv.set_selected(9);
        assert_eq!(inv.selected(), 3, "越界 selected 必须忽略");
    }

    #[test]
    fn save_load_roundtrip_and_seed_isolation() {
        let dir = std::env::temp_dir().join(format!("mc-p18-inv-{}-rt", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut inv = PlayerInventory::new();
        inv.set_selected(2);
        inv.add_item(55, 5);
        inv.slots[37] = Some((1, 64));
        inv.next_state();
        assert_eq!(inv.save_to_dir(&dir, "uuid-tag", 42).unwrap(), 2);
        let (back, is_new) = PlayerInventory::load_from_dir(&dir, "uuid-tag", 42).unwrap();
        assert!(!is_new);
        assert_eq!(back.selected(), 2);
        assert_eq!(back.get(36), Some((55, 5)));
        assert_eq!(back.get(37), Some((1, 64)));
        // seed 隔离：换 seed 读到新背包。
        let (_, is_new2) = PlayerInventory::load_from_dir(&dir, "uuid-tag", 43).unwrap();
        assert!(is_new2);
        // 缺文件=新背包。
        let (_, is_new3) = PlayerInventory::load_from_dir(&dir, "nobody", 42).unwrap();
        assert!(is_new3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_save_errors() {
        let dir = std::env::temp_dir().join(format!("mc-p18-inv-{}-bad", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(PlayerInventory::save_path(&dir, "u", 9), b"NOPE").unwrap();
        assert!(PlayerInventory::load_from_dir(&dir, "u", 9).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P31：XP 与背包同文件 roundtrip；v1 旧存档没有 XP 字段，读回缺省 0。
    #[test]
    fn xp_save_load_roundtrip_and_legacy_version() {
        let dir = std::env::temp_dir().join(format!("mc-p31-xp-{}-rt", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut inv = PlayerInventory::new();
        inv.add_item(55, 3);
        assert_eq!(inv.add_experience(5), 5);
        assert_eq!(inv.add_experience(3), 8);
        assert_eq!(inv.xp_level(), 1, "level 1 starts at 7 total XP");
        inv.save_to_dir(&dir, "xp-user", 77).unwrap();
        let (back, is_new) = PlayerInventory::load_from_dir(&dir, "xp-user", 77).unwrap();
        assert!(!is_new);
        assert_eq!(back.total_xp(), 8);
        assert_eq!(back.get(36), Some((55, 3)), "add_item fills the hotbar first");
        // 手写一份 v1 文件（魔数/版本1/selected/state/0 条目，无 XP 尾字段）。
        let mut legacy = b"P18I".to_vec();
        legacy.push(1);
        legacy.push(0);
        legacy.extend_from_slice(&1i32.to_be_bytes());
        legacy.extend_from_slice(&0u16.to_be_bytes());
        std::fs::write(PlayerInventory::save_path(&dir, "legacy", 77), &legacy).unwrap();
        let (old, is_new) = PlayerInventory::load_from_dir(&dir, "legacy", 77).unwrap();
        assert!(!is_new && old.total_xp() == 0, "legacy v1 save must default XP to 0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn click_pickup_take_place_merge_swap() {
        let mut inv = PlayerInventory::new();
        inv.slots[9] = Some((55, 10));
        // 左键拿起整栈。
        assert!(inv.apply_click(9, 0, 0));
        assert_eq!(inv.get(9), None);
        assert_eq!(inv.cursor(), Some((55, 10)));
        // 左键放到空槽。
        assert!(inv.apply_click(37, 0, 0));
        assert_eq!(inv.get(37), Some((55, 10)));
        assert_eq!(inv.cursor(), None);
        // 同物合并：槽 37 有 10，手持 60 → 槽满 64，手持剩 6。
        inv.cursor = Some((55, 60));
        assert!(inv.apply_click(37, 0, 0));
        assert_eq!(inv.get(37), Some((55, 64)));
        assert_eq!(inv.cursor(), Some((55, 6)));
        // 异物交换。
        inv.slots[38] = Some((1, 3));
        assert!(inv.apply_click(38, 0, 0));
        assert_eq!(inv.get(38), Some((55, 6)));
        assert_eq!(inv.cursor(), Some((1, 3)));
        // 合成/盔甲槽不碰。
        assert!(!inv.apply_click(0, 0, 0));
        assert!(!inv.apply_click(5, 0, 0));
        // 窗外点击是空操作。
        assert!(!inv.apply_click(-999, 0, 0));
        assert!(!inv.apply_click(46, 0, 0));
    }

    #[test]
    fn click_pickup_right_place_one_take_half() {
        let mut inv = PlayerInventory::new();
        // 右键放一：手持 5 → 槽 1，手持 4。
        inv.cursor = Some((55, 5));
        assert!(inv.apply_click(37, 1, 0));
        assert_eq!(inv.get(37), Some((55, 1)));
        assert_eq!(inv.cursor(), Some((55, 4)));
        // 右键同物再放一。
        assert!(inv.apply_click(37, 1, 0));
        assert_eq!(inv.get(37), Some((55, 2)));
        assert_eq!(inv.cursor(), Some((55, 3)));
        // 右键拿半：槽 9 有 7 → 手持 4，槽剩 3。
        inv.cursor = None;
        inv.slots[9] = Some((62, 7));
        assert!(inv.apply_click(9, 1, 0));
        assert_eq!(inv.cursor(), Some((62, 4)));
        assert_eq!(inv.get(9), Some((62, 3)));
        // 右键异物不碰。
        inv.cursor = Some((55, 1));
        assert!(!inv.apply_click(9, 1, 0));
        assert_eq!(inv.cursor(), Some((55, 1)));
    }

    #[test]
    fn click_swap_hotbar_and_quick_move() {
        let mut inv = PlayerInventory::new();
        inv.slots[9] = Some((55, 5));
        inv.slots[36] = Some((1, 64));
        // 数字键 1：槽 9 <-> 快捷栏 37（空）= 搬过去。
        assert!(inv.apply_click(9, 1, 2));
        assert_eq!(inv.get(9), None);
        assert_eq!(inv.get(37), Some((55, 5)));
        // Shift 点击快捷栏 37 → 搬回主包（先合并后进空槽：9 空）。
        assert!(inv.apply_click(37, 0, 1));
        assert_eq!(inv.get(37), None);
        assert_eq!(inv.get(9), Some((55, 5)));
        // Shift 点击主包 9（cobble×3）：快捷栏无同物，进首个空槽 37。
        inv.slots[9] = Some((62, 3));
        assert!(inv.apply_click(9, 0, 1));
        assert_eq!(inv.get(9), None);
        assert_eq!(inv.get(37), Some((62, 3)));
        // 越界 button 拒绝。
        assert!(!inv.apply_click(9, 9, 2));
        // 未知类型拒绝。
        assert!(!inv.apply_click(9, 0, 9));
    }

    /// P40：合成格可 PICKUP，结果槽点击扣原料；9-45 语义不变。
    #[test]
    fn crafting_grid_pickup_and_result_consumes_inputs() {
        let mut inv = PlayerInventory::new();
        // 从主包拿一个原木放进合成格 1。
        inv.slots[9] = Some((crate::drops::ITEM_OAK_LOG, 1));
        assert!(inv.apply_click(9, 0, 0));
        assert!(inv.apply_click(1, 0, 0));
        assert_eq!(inv.get(1), Some((crate::drops::ITEM_OAK_LOG, 1)));
        // 点结果槽 → 4 木板进光标、原料清空。
        assert_eq!(inv.craft_result(0), Some((crate::drops::ITEM_OAK_PLANKS, 4)));
        assert_eq!(inv.cursor(), Some((crate::drops::ITEM_OAK_PLANKS, 4)));
        assert_eq!(inv.get(1), None);
        // 结果槽与盔甲槽仍不可直接 PICKUP；合成格仍不能 THROW/SWAP/QUICK_MOVE。
        assert!(!inv.apply_click(0, 0, 0));
        assert!(!inv.apply_click(5, 0, 0));
        // 空合成格可用光标里的木板放入/取回（只扩展 PICKUP，不碰盔甲/快捷栏语义）。
        assert!(inv.apply_click(2, 0, 0));
        assert_eq!(inv.get(2), Some((crate::drops::ITEM_OAK_PLANKS, 4)));
        assert!(inv.apply_click(2, 0, 0));
        assert_eq!(inv.get(2), None);
        assert!(inv.apply_click(9, 0, 0), "stow cursor back to main inventory");
        assert_eq!(inv.cursor(), None);
        assert!(!inv.apply_click(1, 0, 1));
        assert!(!inv.apply_click(1, 0, 2));
        assert_eq!(inv.throw_from_slot(1, 0), None);
    }

    /// P40：生存状态随背包存档 roundtrip，v1/v2 旧档读回满状态。
    #[test]
    fn survival_save_load_roundtrip_and_legacy_defaults() {
        let dir = std::env::temp_dir().join(format!("mc-p40-surv-{}-rt", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut inv = PlayerInventory::new();
        inv.add_item(55, 3);
        inv.set_survival(crate::survival::SurvivalState { health: 11.5, food: 7, saturation: 2.0 });
        inv.save_to_dir(&dir, "surv-user", 88).unwrap();
        let (back, _) = PlayerInventory::load_from_dir(&dir, "surv-user", 88).unwrap();
        assert_eq!(back.survival(), inv.survival());
        // v2 旧档（XP=5，无生存字段）仍可读，生存取默认满值。
        let mut v2 = b"P18I".to_vec();
        v2.push(2);
        v2.push(0);
        v2.extend_from_slice(&1i32.to_be_bytes());
        v2.extend_from_slice(&0u16.to_be_bytes());
        v2.extend_from_slice(&5i32.to_be_bytes());
        std::fs::write(PlayerInventory::save_path(&dir, "v2", 88), &v2).unwrap();
        let (old, _) = PlayerInventory::load_from_dir(&dir, "v2", 88).unwrap();
        assert_eq!(old.total_xp(), 5);
        assert_eq!(old.survival(), crate::survival::SurvivalState::full());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P53：v4 疲劳/设置 roundtrip；v3 旧档缺 6B 时按 0/survival/none 兼容读。
    #[test]
    fn p53_exhaustion_and_settings_save_load_legacy_v3() {
        let dir = std::env::temp_dir().join(format!("mc-p53-inv-{}-rt", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut inv = PlayerInventory::new();
        inv.set_exhaustion(2.25);
        inv.set_persisted_gamemode(SETTINGS_GAMEMODE_CREATIVE);
        inv.set_persisted_tool_enchant(SETTINGS_ENCHANT_SILK_TOUCH);
        inv.save_to_dir(&dir, "p53", 99).unwrap();
        let (back, _) = PlayerInventory::load_from_dir(&dir, "p53", 99).unwrap();
        assert_eq!(back.exhaustion(), 2.25);
        assert_eq!(back.persisted_gamemode(), SETTINGS_GAMEMODE_CREATIVE);
        assert_eq!(back.persisted_tool_enchant(), SETTINGS_ENCHANT_SILK_TOUCH);
        // 手写 v3（XP + 生存 12B，无 P53 尾部）。
        let mut v3 = b"P18I".to_vec();
        v3.push(3);
        v3.push(0);
        v3.extend_from_slice(&1i32.to_be_bytes());
        v3.extend_from_slice(&0u16.to_be_bytes());
        v3.extend_from_slice(&7i32.to_be_bytes());
        v3.extend_from_slice(&9.5f32.to_be_bytes());
        v3.extend_from_slice(&6i32.to_be_bytes());
        v3.extend_from_slice(&1.5f32.to_be_bytes());
        std::fs::write(PlayerInventory::save_path(&dir, "old", 99), &v3).unwrap();
        let (old, _) = PlayerInventory::load_from_dir(&dir, "old", 99).unwrap();
        assert_eq!(old.total_xp(), 7);
        assert_eq!(old.exhaustion(), 0.0);
        assert_eq!(old.persisted_gamemode(), SETTINGS_GAMEMODE_SURVIVAL);
        assert_eq!(old.persisted_tool_enchant(), SETTINGS_ENCHANT_NONE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cursor_not_persisted_but_stowed_on_unload() {
        let dir = std::env::temp_dir().join(format!("mc-p18-inv-{}-cur", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut inv = PlayerInventory::new();
        inv.add_item(55, 5);
        inv.cursor = Some((62, 3));
        inv.next_state();
        inv.save_to_dir(&dir, "u", 9).unwrap();
        let (back, _) = PlayerInventory::load_from_dir(&dir, "u", 9).unwrap();
        assert_eq!(back.cursor(), None, "cursor 不进存档");
        // stow 把手持收回背包。
        inv.stow_cursor();
        assert_eq!(inv.cursor(), None);
        assert!(inv.to_protocol_slots().iter().any(|s| *s != mc_protocol::packets::Slot::empty() && s.item_id == 62));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- P56：26.3 jar 对齐（InventoryMenu / AbstractContainerMenu） ----

    /// jar `InventoryMenu`: RESULT_SLOT=0 / CRAFT_SLOT_START=1..4 / INV 9..36 /
    /// USE_ROW 36..45 / SHIELD_SLOT=45；`AbstractContainerMenu.SLOT_CLICKED_OUTSIDE=-999`。
    #[test]
    fn p56_window0_slot_layout_matches_inventory_menu() {
        assert_eq!(crate::crafting::RESULT_SLOT, 0);
        assert_eq!(crate::crafting::GRID_SIZE, 4);
        assert_eq!(MAIN_START, 9);
        assert_eq!(MAIN_END, 35);
        assert_eq!(HOTBAR_START, 36);
        assert_eq!(HOTBAR_END, 44);
        assert_eq!(SLOT_COUNT, 46);
        assert_eq!(crate::crafting::CRAFT9_RESULT_SLOT, 0);
        assert_eq!(crate::crafting::CRAFT9_WINDOW_ID, 1);
    }

    /// jar `CraftingMenu.slotChangedCraftingGrid`：结果槽由服务端算出并同步。
    #[test]
    fn p56_result_protocol_slot_is_server_computed() {
        let mut inv = PlayerInventory::new();
        assert_eq!(inv.craft_result_protocol_slot(), mc_protocol::packets::Slot::empty());
        inv.slots[1] = Some((crate::drops::ITEM_OAK_LOG, 1));
        let slot = inv.craft_result_protocol_slot();
        assert_eq!(slot.item_id, crate::drops::ITEM_OAK_PLANKS);
        assert_eq!(slot.count, 4);
        assert_eq!(inv.get(0), None, "结果不写进背包槽 0（只读结果槽）");
    }

    /// jar `ResultSlot.mayPlace()==false`：结果槽只能由配方引擎写入。
    #[test]
    fn p56_result_slot_rejects_hand_placement() {
        let mut inv = PlayerInventory::new();
        inv.set_cursor(Some((crate::drops::ITEM_OAK_LOG, 1)));
        // PICKUP / SWAP / QUICK_MOVE / THROW 都不能把手持物放进槽 0。
        assert!(!inv.apply_click(0, 0, 0));
        assert!(!inv.apply_click(0, 0, 1));
        assert!(!inv.apply_click(0, 0, 2));
        assert_eq!(inv.get(0), None);
        assert_eq!(inv.cursor(), Some((crate::drops::ITEM_OAK_LOG, 1)));
        inv.set_cursor(None);
    }

    /// jar `AbstractContainerMenu.doClick` QUICK_MOVE 循环 + `ResultSlot.onTake`：
    /// SHIFT 结果槽把连续合成的产物入包并逐格扣原料（背包 2×2 不补料）。
    #[test]
    fn p56_shift_result_crafts_repeatedly_and_stows() {
        let mut inv = PlayerInventory::new();
        // 两格各 1 木板 → 木棍的规范槽型（1+1）；单格 2 木板则是压力板。
        inv.slots[1] = Some((crate::drops::ITEM_OAK_PLANKS, 1));
        inv.slots[2] = Some((crate::drops::ITEM_OAK_PLANKS, 1));
        assert_eq!(inv.craft_result_quick_move(), 1, "two planks -> four sticks");
        assert_eq!(inv.slots[1], None);
        assert_eq!(inv.slots[2], None);
        assert_eq!(inv.cursor(), None, "SHIFT never touches the cursor");
        let sticks = crate::crafting::RECIPES.iter().find(|r| r.name == "stick").unwrap().result.0;
        assert_eq!(inv.get(36), Some((sticks, 4)), "result lands in hotbar first");
        assert_eq!(inv.craft_result_quick_move(), 0, "no inputs left, no extra craft");
        // 子集语义：单格 2 原木也能做（连做 2 次，耗尽为止；原版 SHIFT 同理）。
        inv.slots[1] = Some((crate::drops::ITEM_OAK_LOG, 2));
        assert_eq!(inv.craft_result_quick_move(), 2);
        assert_eq!(inv.slots[1], None);
        let planks_total: i32 = (9..=44)
            .filter_map(|s| inv.get(s))
            .filter(|(id, _)| *id == crate::drops::ITEM_OAK_PLANKS)
            .map(|(_, c)| c)
            .sum();
        assert_eq!(planks_total, 8);
    }

    /// jar `AbstractContainerMenu.doClick` SWAP 只在目标槽为空时把结果放进去。
    #[test]
    fn p56_swap_result_into_empty_hotbar_or_offhand() {
        let mut inv = PlayerInventory::new();
        inv.slots[1] = Some((crate::drops::ITEM_OAK_LOG, 1));
        inv.set_cursor(Some((crate::drops::ITEM_STICK, 1)));
        assert!(!inv.craft_result_swap(0), "occupied cursor never crafts via SWAP");
        inv.set_cursor(None);
        // button 0 把 4 木板放进快捷栏 36。
        assert!(inv.craft_result_swap(0));
        assert_eq!(inv.get(36), Some((crate::drops::ITEM_OAK_PLANKS, 4)));
        assert_eq!(inv.get(1), None);
        assert_eq!(inv.cursor(), None);
        // 目标槽非空 → no-op，原料保留。
        inv.slots[1] = Some((crate::drops::ITEM_OAK_LOG, 1));
        assert!(!inv.craft_result_swap(0));
        assert_eq!(inv.slots[1], Some((crate::drops::ITEM_OAK_LOG, 1)));
        // button 40 = 副手槽 45。
        assert!(inv.craft_result_swap(40));
        assert_eq!(inv.get(45), Some((crate::drops::ITEM_OAK_PLANKS, 4)));
        assert!(!inv.craft_result_swap(9));
    }

    /// jar `InventoryMenu.quickMoveStack` 对 1..=4 的 SHIFT：`moveItemStackTo(9, 45, false)`。
    #[test]
    fn p56_quick_move_grid_slot_into_main_or_hotbar() {
        let mut inv = PlayerInventory::new();
        inv.slots[1] = Some((crate::drops::ITEM_OAK_LOG, 3));
        assert!(inv.quick_move_grid_slot(1));
        assert_eq!(inv.get(1), None);
        assert_eq!(inv.get(36), Some((crate::drops::ITEM_OAK_LOG, 3)));
        // 同物合并不新开槽。
        inv.slots[2] = Some((crate::drops::ITEM_OAK_LOG, 2));
        assert!(inv.quick_move_grid_slot(2));
        assert_eq!(inv.get(36), Some((crate::drops::ITEM_OAK_LOG, 5)));
        assert_eq!(inv.get(2), None);
        assert!(!inv.quick_move_grid_slot(1), "empty grid slot does nothing");
        assert!(!inv.quick_move_grid_slot(5), "armor slot is not a craft grid slot");
    }

    /// jar `AbstractContainerMenu.incrementStateId()`：`(state + 1) & Short.MAX_VALUE`。
    #[test]
    fn p56_state_id_wraps_at_short_max() {
        let mut inv = PlayerInventory::new();
        assert_eq!(inv.state_id(), 1);
        inv.set_state_id(0x7fff);
        assert_eq!(inv.next_state(), 0);
        assert_eq!(inv.next_state(), 1);
        inv.set_state_id(123);
        assert_eq!(inv.next_state(), 124);
    }
}
