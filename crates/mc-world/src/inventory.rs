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
const SAVE_MAGIC: [u8; 4] = *b"P18I";
const SAVE_VERSION: u8 = 1;
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
}

impl Default for PlayerInventory {
    fn default() -> Self {
        Self { slots: [None; SLOT_COUNT], selected: 0, state_id: 1, cursor: None }
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

    /// 集装箱点击应用（只处理玩家背包窗口的 9..=45 槽；返回 true=状态变了）：
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

    /// 可操作槽：主背包 9-35、快捷栏 36-44、副手 45；合成/盔甲 0-8 不碰。
    fn play_slot(&self, slot: i16) -> Option<usize> {
        if (9..=45).contains(&slot) {
            Some(slot as usize)
        } else {
            None
        }
    }

    fn click_pickup(&mut self, slot: i16, button: i8) -> bool {
        let idx = match self.play_slot(slot) {
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

    pub fn get(&self, idx: usize) -> Option<(i32, i32)> {
        self.slots.get(idx).copied().flatten()
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

    /// 落盘（tmp+rename 原子替换）：魔数/版本/selected/state/条目(槽u8+物品i32+数量i32)。
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
        let mut buf = Vec::with_capacity(8 + entries.len() * 9);
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
        if head[4] != SAVE_VERSION {
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
