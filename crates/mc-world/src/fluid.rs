//! P46 v2 流体/游泳的纯规则（不依赖网络、不做每连接物理）。
//!
//! 26.3 的 `minecraft:water` 是**单个 block**，`level=0..15` 对应 16 个连续
//! blockstate（89..104）。本模块所有 state 都用 `state_index_id` 查生成的
//! `BLOCK_STATES` 表，绝不写死 89+level。
//!
//! 简化规则（与原版差异，需真机 26.3 复验）：
//! - `level 0` = source（静止源），`level 1..=15` = flowing（数值越大越弱）；
//! - 只向 `air` 扩散；不做原版的压力均衡/粘滞/侧入/双源合成 source；
//! - 每格向外扩.level+1，水平/向下同规则，最大传播距离 [`FLOW_SPREAD_MAX`]；
//! - source 每拍会重新填满相邻空格（挖开容器后水可恢复），失去 inflow 的
//!   流水在邻格重算时退成空气（堵住容器后水可停止）；
//! - 游泳不模拟每连接物理：客户端仍发 C2S 移动包，服务端只做水中判定、
//!   坠落伤害豁免与最小位置合法性钳制。

use crate::{AIR_BLOCK_ID, EYE_HEIGHT, MIN_Y, WORLD_TOP_Y};

/// 水平/向下最多传播 8 格（与常见 flowing level 1..8 口径一致；26.3 需真机复验）。
pub const FLOW_SPREAD_MAX: u8 = 8;
/// 水的最大 level（26.3 `properties.json::level` min=0 max=15）。
pub const WATER_LEVEL_MAX: u8 = 15;

/// `level` → 真实 blockstate ID（编译期查表，越界钳到 15）。
pub const fn water_state_id(level: u8) -> u16 {
    crate::state_index_id(
        mc_registry::BLOCK_STATES,
        "minecraft:water",
        (if level > WATER_LEVEL_MAX { WATER_LEVEL_MAX } else { level }) as u16,
    )
}

/// source（level 0）的真实 blockstate ID。
pub const fn water_source_id() -> u16 {
    water_state_id(0)
}

/// 是否为 26.3 的 16 个水 state 之一（source + flowing）。
pub const fn is_water(id: u16) -> bool {
    let base = water_source_id();
    id >= base && id < base + 16
}

/// 水的 level（非水返回 `None`）。
pub const fn water_level(id: u16) -> Option<u8> {
    if is_water(id) {
        Some((id - water_source_id()) as u8)
    } else {
        None
    }
}

/// 流动方向：向下或水平（简化规则同衰减，保留枚举便于测试/扩展）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowRule {
    Down,
    Horizontal,
}

/// 从 `from` level 流出一格的新 level；超出传播距离返回 `None`。
pub const fn flow_out_level(from: u8, _rule: FlowRule) -> Option<u8> {
    if from >= FLOW_SPREAD_MAX {
        return None;
    }
    let next = from + 1;
    if next <= FLOW_SPREAD_MAX {
        Some(next)
    } else {
        None
    }
}

/// 邻格是否能作为本格的 inflow 支撑：来自下方的不算（水不会向上流），
/// 其余方向 level 严格更小即可（source=0 支撑任意流水）。
pub const fn supports_inflow(self_level: u8, neighbor_dy: i32, neighbor_level: u8) -> bool {
    neighbor_dy >= 0 && neighbor_level < self_level
}

/// 游泳判定结果：`in_water` 决定是否豁免坠落伤害，`min_feet_y` 是位置合法性下界。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SwimState {
    pub in_water: bool,
    /// 脚下最高非流体方块顶面（玩家脚 y 不得小于它）。
    pub min_feet_y: f64,
}

/// 纯函数游泳判定：`get` 为世界查询闭包（生产传 `MutableWorld::get`）。
/// 水中判定取脚格、脚上一格、眼格任一为水；`min_feet_y` 只用于位置合法性，
/// **不**在这里改速度/浮力——那属于客户端预测，服务端不逐连接模拟。
pub fn swim_state<F: Fn(i32, i32, i32) -> u16>(get: F, x: f64, y: f64, z: f64) -> SwimState {
    let bx = x.floor() as i32;
    let bz = z.floor() as i32;
    let feet = y.floor() as i32;
    let eye = (y + EYE_HEIGHT).floor() as i32;
    let in_water = is_water(get(bx, feet, bz))
        || is_water(get(bx, feet + 1, bz))
        || is_water(get(bx, eye, bz));
    if !in_water {
        return SwimState { in_water: false, min_feet_y: y };
    }
    let mut top = MIN_Y - 1;
    let scan = feet.min(WORLD_TOP_Y);
    for yy in (MIN_Y..=scan).rev() {
        let id = get(bx, yy, bz);
        if id != AIR_BLOCK_ID && !is_water(id) {
            top = yy;
            break;
        }
    }
    SwimState { in_water: true, min_feet_y: (top + 1) as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AIR_BLOCK_ID, STONE_BLOCK_ID, WATER_BLOCK_ID};

    #[test]
    fn water_states_are_the_real_26_3_table() {
        assert_eq!(water_source_id(), WATER_BLOCK_ID);
        assert_eq!(water_state_id(15), WATER_BLOCK_ID + 15);
        for level in 0..=15 {
            let id = water_state_id(level);
            assert!(is_water(id));
            assert_eq!(water_level(id), Some(level));
        }
        assert!(!is_water(AIR_BLOCK_ID));
        assert!(!is_water(STONE_BLOCK_ID));
        assert_eq!(water_level(STONE_BLOCK_ID), None);
        assert_eq!(water_state_id(99), water_state_id(15), "out of range clamps");
    }

    #[test]
    fn flow_decays_one_per_cell_and_stops_at_limit() {
        assert_eq!(flow_out_level(0, FlowRule::Horizontal), Some(1));
        assert_eq!(flow_out_level(0, FlowRule::Down), Some(1));
        assert_eq!(flow_out_level(1, FlowRule::Horizontal), Some(2));
        assert_eq!(flow_out_level(7, FlowRule::Horizontal), Some(8));
        assert_eq!(flow_out_level(8, FlowRule::Horizontal), None);
    }

    #[test]
    fn inflow_cannot_come_from_below_and_needs_lower_level() {
        assert!(supports_inflow(1, 0, 0), "source beside flowing water feeds it");
        assert!(supports_inflow(2, 1, 1), "water above feeds falling column");
        assert!(!supports_inflow(1, -1, 0), "water below never feeds upward");
        assert!(!supports_inflow(1, 0, 2), "equal/stronger neighbor gives no inflow");
    }

    #[test]
    fn swim_state_detects_water_and_clamps_to_support() {
        // 脚在 y=3、水在 3..5、石头地表 y=2 → 脚不得小于 3。
        let get = |_x, y, _z| if y <= 2 { STONE_BLOCK_ID } else if y <= 5 { water_source_id() } else { AIR_BLOCK_ID };
        let s = swim_state(get, 4.5, 3.4, 9.5);
        assert!(s.in_water);
        assert_eq!(s.min_feet_y, 3.0);
        let below = swim_state(get, 4.5, 0.5, 9.5);
        assert!(!below.in_water);
        assert_eq!(below.min_feet_y, 0.5);
        // 眼格在水中也算（脚在空气、眼睛没入水面）。
        let eye = |_x, y, _z| if y == 4 { water_source_id() } else { AIR_BLOCK_ID };
        assert!(swim_state(eye, 0.5, 3.0, 0.5).in_water);
    }
}
