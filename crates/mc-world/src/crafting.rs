//! crafting: P40 2×2 玩家背包合成的纯逻辑（配方 + 匹配 + 消耗），只兼容 26.3/777。
//!
//! 所有物品 ID 均由 `mc_registry::ITEMS`（`data/777/items.json` 生成表）在编译期查得，
//! 与 `drops.rs` 同口径；不手写/猜物品 ID。本轮只做玩家背包 2×2（槽 1–4 输入、0 输出），
//! 3×3 工作台 / block entity 明确不做（见 HANDOFF 任务说明）。
//!
//! 配方为**无序精确匹配**：把 4 个合成格内的物品按 ID 计数，必须与某条配方的
//! 需求计数完全一致（不多不少）；输出栈直接返回给调用方，由网络层放光标/背包。

use crate::registry_id;

/// 合成格数量（背包 1..=4）。
pub const GRID_SIZE: usize = 4;
/// 合成结果槽（背包 0）。
pub const RESULT_SLOT: usize = 0;

/// 单条无序配方：`needs` 为 (物品 ID, 数量)，`result` 为 (物品 ID, 数量)。
#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    pub needs: &'static [(i32, i32)],
    pub result: (i32, i32),
    /// 便于诊断/测试的名称（不参与匹配）。
    pub name: &'static str,
}

// 物品 ID 全部来自生成表（括号里是 data/777/items.json 的实测值，改数据后编译期自动漂移）。
const OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_log") as i32; // 163
const OAK_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_planks") as i32; // 63
const STICK: i32 = registry_id(mc_registry::ITEMS, "minecraft:stick") as i32; // 1060
const CRAFTING_TABLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:crafting_table") as i32; // 405
const COBBLESTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobblestone") as i32; // 62
const WOODEN_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_pickaxe") as i32; // 1027
const STONE_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_pickaxe") as i32; // 1037
const WHEAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:wheat") as i32; // 1066
const BREAD: i32 = registry_id(mc_registry::ITEMS, "minecraft:bread") as i32; // 1067
const COAL: i32 = registry_id(mc_registry::ITEMS, "minecraft:coal") as i32; // 1010
const TORCH: i32 = registry_id(mc_registry::ITEMS, "minecraft:torch") as i32; // 395

/// MVP 配方表（均为原版 2×2 可合成口径）：
/// 原木→4 木板；2 木板→4 木棍；4 木板→工作台；
/// 3 木板+2 木棍→木镐；3 圆石+2 木棍→石镐；3 小麦→面包；煤+木棍→4 火把。
pub const RECIPES: &[Recipe] = &[
    Recipe { needs: &[(OAK_LOG, 1)], result: (OAK_PLANKS, 4), name: "oak_planks" },
    Recipe { needs: &[(OAK_PLANKS, 2)], result: (STICK, 4), name: "stick" },
    Recipe { needs: &[(OAK_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "crafting_table" },
    Recipe { needs: &[(OAK_PLANKS, 3), (STICK, 2)], result: (WOODEN_PICKAXE, 1), name: "wooden_pickaxe" },
    Recipe { needs: &[(COBBLESTONE, 3), (STICK, 2)], result: (STONE_PICKAXE, 1), name: "stone_pickaxe" },
    Recipe { needs: &[(WHEAT, 3)], result: (BREAD, 1), name: "bread" },
    Recipe { needs: &[(COAL, 1), (STICK, 1)], result: (TORCH, 4), name: "torch" },
];

/// 把合成格展平成 `(id, count)` 列表（跳过空槽；未合并，匹配时按 ID 累计）。
pub fn grid_stacks(grid: [Option<(i32, i32)>; GRID_SIZE]) -> Vec<(i32, i32)> {
    grid.into_iter().flatten().collect()
}

/// 无序精确匹配：返回命中的配方；格内多/少任意一件都视为无配方（结果槽保持空）。
pub fn match_recipe(grid: [Option<(i32, i32)>; GRID_SIZE]) -> Option<&'static Recipe> {
    let stacks = grid_stacks(grid);
    RECIPES.iter().find(|recipe| {
        // 每种需求 ID 的实际数量必须恰好相等；且不能出现配方外的 ID。
        for (id, need) in recipe.needs {
            let have: i32 = stacks.iter().filter(|(i, _)| i == id).map(|(_, c)| *c).sum();
            if have != *need {
                return false;
            }
        }
        stacks.iter().all(|(id, _)| recipe.needs.iter().any(|(n, _)| n == id))
    })
}

/// 配方命中后按 ID 从合成格扣除原料（每格最多取自己数量；返回是否全部扣成功）。
/// 调用方应先 `match_recipe`；不匹配时不要调用，避免误扣。
pub fn consume_grid(grid: &mut [Option<(i32, i32)>; GRID_SIZE], recipe: &Recipe) -> bool {
    for (id, need) in recipe.needs {
        let mut left = *need;
        for slot in grid.iter_mut() {
            if left <= 0 {
                break;
            }
            if let Some((sid, count)) = slot {
                if *sid != *id {
                    continue;
                }
                let take = (*count).min(left);
                *count -= take;
                left -= take;
                if *count <= 0 {
                    *slot = None;
                }
            }
        }
        if left > 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipe_item_ids_come_from_generated_table() {
        for (name, expected, id) in [
            ("minecraft:oak_log", 163, OAK_LOG),
            ("minecraft:oak_planks", 63, OAK_PLANKS),
            ("minecraft:stick", 1060, STICK),
            ("minecraft:crafting_table", 405, CRAFTING_TABLE),
            ("minecraft:cobblestone", 62, COBBLESTONE),
            ("minecraft:wooden_pickaxe", 1027, WOODEN_PICKAXE),
            ("minecraft:stone_pickaxe", 1037, STONE_PICKAXE),
            ("minecraft:wheat", 1066, WHEAT),
            ("minecraft:bread", 1067, BREAD),
            ("minecraft:coal", 1010, COAL),
            ("minecraft:torch", 395, TORCH),
        ] {
            assert_eq!(id as u32, expected, "wrong item ID for {name}");
            assert_eq!(mc_registry::item_id(name), Some(expected));
        }
    }

    #[test]
    fn match_and_consume_basic_recipes() {
        let mut grid = [None, Some((OAK_LOG, 1)), None, None];
        let r = match_recipe(grid).expect("one log must make planks");
        assert_eq!(r.result, (OAK_PLANKS, 4));
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [None; GRID_SIZE]);

        // 2 木板 → 4 木棍（无序，位置无关）。
        let grid = [Some((OAK_PLANKS, 1)), None, None, Some((OAK_PLANKS, 1))];
        assert_eq!(match_recipe(grid).unwrap().result, (STICK, 4));

        // 3 木板 + 2 木棍 → 木镐（堆叠也算数量）。
        let grid = [Some((OAK_PLANKS, 3)), Some((STICK, 2)), None, None];
        assert_eq!(match_recipe(grid).unwrap().result, (WOODEN_PICKAXE, 1));

        // 3 小麦 → 面包；煤 + 木棍 → 火把。
        assert_eq!(match_recipe([Some((WHEAT, 3)), None, None, None]).unwrap().result, (BREAD, 1));
        assert_eq!(match_recipe([Some((COAL, 1)), Some((STICK, 1)), None, None]).unwrap().result, (TORCH, 4));
    }

    #[test]
    fn extra_or_missing_items_never_match() {
        assert!(match_recipe([None; GRID_SIZE]).is_none());
        assert!(match_recipe([Some((OAK_LOG, 2)), None, None, None]).is_none());
        // 多一个无关物品 → 原木配方不成立（避免白送木板）。
        assert!(match_recipe([Some((OAK_LOG, 1)), Some((STICK, 1)), None, None]).is_none());
        // 少一件原料 → 石镐不成立。
        assert!(match_recipe([Some((COBBLESTONE, 3)), Some((STICK, 1)), None, None]).is_none());
    }

    #[test]
    fn consume_grid_takes_only_matching_stacks() {
        let mut grid = [Some((OAK_PLANKS, 3)), Some((STICK, 2)), None, None];
        let r = match_recipe(grid).unwrap();
        assert_eq!(r.name, "wooden_pickaxe");
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [None; GRID_SIZE]);
    }
}
