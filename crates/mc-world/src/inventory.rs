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
/// health f32 BE + food i32 BE + saturation f32 BE）。v1/v2 旧档仍可读，
/// XP/生存值分别缺省 0 / 满状态（向后兼容）。
const SAVE_MAGIC: [u8; 4] = *b"P18I";
const SAVE_VERSION: u8 = 3;
const LEGACY_SAVE_VERSION: u8 = 1;
const LEGACY_SAVE_VERSION_V2: u8 = 2;
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

    pub fn next_state(&mut self) -> i32 {
        self.state_id += 1;
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
    /// + 尾部 4 字节总经验（P31 v2）+ 12 字节生存状态（P40 v3）。
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
        let mut buf = Vec::with_capacity(12 + entries.len() * 9 + 4 + 12);
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
        if head[4] == SAVE_VERSION || head[4] == LEGACY_SAVE_VERSION_V2 {
            let mut xp = [0u8; 4];
            f.read_exact(&mut xp).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 经验字段截断 ({e})", path.display()))
            })?;
            inv.total_xp = i32::from_be_bytes(xp).max(0);
        }
        // v3+ 生存三字段；v1/v2 无此字段，生存状态取默认满值（向后兼容）。
        if head[4] == SAVE_VERSION {
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
}
