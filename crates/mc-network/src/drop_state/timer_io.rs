//! P39 计时持久化的 I/O 与启动认领逻辑（`DropState` 的子模块）。
//!
//! 拆出来只为控制单文件行数：所有函数仍是 `DropState` 的方法，内存锁与 redb IO
//! 的顺序、以及 legacy 模式的旧分支完全不变（见各方法文档）。
use super::*;

impl DropState {
    // -----------------------------------------------------------------------
    // P39：timer 持久化小工具。**只能在内存锁释放后调用**（P38 锁纪律：不持锁 IO）。
    // 写失败只 warn：内存跟踪仍是权威，下次登记/推进会重写，不让 IO 拖垮游戏线程。
    // -----------------------------------------------------------------------

    /// 一批 timer 更新 + 当前全局 tick 基线，一次 redb 事务提交。
    pub(super) fn persist_timers(&self, updates: &[TimerRecord]) {
        let Some(store) = &self.store else { return };
        if updates.is_empty() {
            return;
        }
        let tick = self.tick_count.load(Ordering::Relaxed);
        if let Err(e) = store.set_timers(updates, tick) {
            warn!("redb timer save failed seed={} tick={tick} ({e})", self.seed);
        }
    }

    /// 删除若干 timer（挖掉/取消/到期完成）。
    pub(super) fn clear_timers(&self, keys: &[(TimerKind, (i32, i32, i32))]) {
        let Some(store) = &self.store else { return };
        for (kind, pos) in keys {
            if let Err(e) = store.delete_timer(*kind, *pos) {
                warn!("redb timer clear failed seed={} pos={pos:?} ({e})", self.seed);
            }
        }
    }

    pub(super) fn clear_timer(&self, kind: TimerKind, pos: (i32, i32, i32)) {
        self.clear_timers(&[(kind, pos)]);
    }

    /// `PlantKind` + 当前 stage → 持久化口径的 kind/spec（ Fruit 标志在 spec bit7）。
    pub(super) fn plant_timer(
        kind: PlantKind,
        pos: (i32, i32, i32),
        planted_tick: u64,
        stage: u8,
        fruit_done: bool,
    ) -> TimerRecord {
        let (table, spec) = match kind {
            PlantKind::Age(s) => (TimerKind::Crop, timer_spec::age(s, fruit_done)),
            PlantKind::Tree(s) => (TimerKind::Sapling, timer_spec::tree(s)),
            PlantKind::Vertical(s) => (TimerKind::Crop, timer_spec::vertical(s)),
            PlantKind::Side(_) => (TimerKind::Crop, timer_spec::side()),
        };
        TimerRecord { kind: table, pos, planted_tick, stage, spec }
    }

    pub(super) fn plant_timer_kind(kind: PlantKind) -> TimerKind {
        match kind {
            PlantKind::Tree(_) => TimerKind::Sapling,
            _ => TimerKind::Crop,
        }
    }

    /// P39：从 `timers` 认领记录；`retain` 会把同类的记录从待认领队列取走。
    pub(super) fn take_restored(&self, want: impl Fn(&Restored) -> bool) -> Vec<TimerRecord> {
        let mut restored = self.restored_timers.lock().unwrap_or_else(|p| p.into_inner());
        let taken: Vec<TimerRecord> = restored
            .iter()
            .copied()
            .filter(|r| Restored::from_record(r).is_some_and(|p| want(&p)))
            .collect();
        restored.retain(|r| !Restored::from_record(r).is_some_and(|p| want(&p)));
        taken
    }

    /// P22/P33：重启后把存量作物（含小麦）重新纳入生长。
    /// redb 模式优先用 `timers` 的原 planted/stage 续算；没有 timer 的存量方块
    /// （早期存档/写失败）仍按老口径从现在重计时。legacy 模式逐字节不变。
    pub fn adopt_existing_crops(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let restored = self.take_restored(|r| matches!(r, Restored::P33Crop(_)));
        let mut stale = Vec::new();
        {
            let mut crops = self.crops.lock().unwrap_or_else(|p| p.into_inner());
            for rec in restored {
                if let Some(Restored::P33Crop(crop)) = Restored::from_record(&rec) {
                    let (x, y, z) = rec.pos;
                    if mc_world::crop_age_of(world.get(x, y, z)).map(|(c, _)| c.name)
                        == Some(crop.name)
                    {
                        crops.entry(rec.pos).or_insert(CropEntry {
                            planted_tick: rec.planted_tick,
                            stage: rec.stage.min(crop.max_age),
                            crop,
                        });
                    } else {
                        stale.push((TimerKind::Crop, rec.pos));
                    }
                }
            }
            for ((x, y, z), id) in world.crop_blocks() {
                if let Some((crop, age)) = mc_world::crop_age_of(id) {
                    crops
                        .entry((x, y, z))
                        .or_insert(CropEntry { planted_tick: now, stage: age, crop });
                }
            }
        }
        self.clear_timers(&stale);
    }

    /// P37：重启后认领存量新植物。
    /// redb 模式按 `timers` 的原 planted/stage + spec 续算（含 Fruit 标志、可可 facing
    /// 从方块恢复）；没有 timer 的存量植物仍从重启时刻重计时。
    pub fn adopt_existing_plants(&self, world: &MutableWorld) {
        use mc_world::crops::{age_crop_of, cocoa_of, plant_at, vertical_spec_of};
        let now = self.tick_count.load(Ordering::Relaxed);
        let restored = self.take_restored(|r| {
            matches!(
                r,
                Restored::Age(_, _) | Restored::Tree(_) | Restored::Vertical(_) | Restored::Side(_)
            )
        });
        let mut stale = Vec::new();
        {
            let mut plants = self.plants.lock().unwrap_or_else(|p| p.into_inner());
            for rec in restored {
                let Some(parsed) = Restored::from_record(&rec) else { continue };
                let (x, y, z) = rec.pos;
                let id = world.get(x, y, z);
                let entry = match parsed {
                    Restored::Age(spec, fruit_done) => {
                        if age_crop_of(id).map(|(s, _)| s.name) != Some(spec.name) {
                            stale.push((Self::plant_timer_kind(PlantKind::Age(spec)), rec.pos));
                            continue;
                        }
                        Some(PlantEntry {
                            kind: PlantKind::Age(spec),
                            planted_tick: rec.planted_tick,
                            stage: rec.stage.min(spec.max_age),
                            facing: 0,
                            fruit_done,
                        })
                    }
                    Restored::Tree(spec) => {
                        if id != spec.sapling_id {
                            stale.push((TimerKind::Sapling, rec.pos));
                            continue;
                        }
                        Some(PlantEntry {
                            kind: PlantKind::Tree(spec),
                            planted_tick: rec.planted_tick,
                            stage: 0,
                            facing: 0,
                            fruit_done: false,
                        })
                    }
                    Restored::Vertical(spec) => {
                        if vertical_spec_of(id).map(|s| s.name) != Some(spec.name)
                            || vertical_spec_of(world.get(x, y - 1, z)).map(|s| s.name) == Some(spec.name)
                        {
                            stale.push((TimerKind::Crop, rec.pos));
                            continue;
                        }
                        Some(PlantEntry {
                            kind: PlantKind::Vertical(spec),
                            planted_tick: rec.planted_tick,
                            stage: rec.stage.min(spec.max_height - 1),
                            facing: 0,
                            fruit_done: false,
                        })
                    }
                    Restored::Side(spec) => {
                        if cocoa_of(id).is_none() {
                            stale.push((TimerKind::Crop, rec.pos));
                            continue;
                        }
                        Some(PlantEntry {
                            kind: PlantKind::Side(spec),
                            planted_tick: rec.planted_tick,
                            stage: rec.stage.min(spec.max_age),
                            facing: cocoa_of(id).map(|(_, _, f)| f).unwrap_or(0),
                            fruit_done: false,
                        })
                    }
                    _ => None,
                };
                if let Some(entry) = entry {
                    plants.entry(rec.pos).or_insert(entry);
                }
            }
            // 没有 timer 的存量植物沿用旧口径：从重启时刻重新计时。
            for ((x, y, z), id) in world.plant_blocks() {
                let Some((kind, age)) = plant_at(id) else { continue };
                // 竖向只跟踪基部，并把已长高度写进 stage。
                if let PlantKind::Vertical(spec) = kind {
                    if vertical_spec_of(world.get(x, y - 1, z)).map(|s| s.name) == Some(spec.name) {
                        continue;
                    }
                    let mut h = 0u8;
                    while h < spec.max_height - 1
                        && vertical_spec_of(world.get(x, y + h as i32 + 1, z)).map(|s| s.name) == Some(spec.name)
                    {
                        h += 1;
                    }
                    plants.entry((x, y, z)).or_insert(PlantEntry {
                        kind,
                        planted_tick: now,
                        stage: h,
                        facing: 0,
                        fruit_done: false,
                    });
                    continue;
                }
                let facing = cocoa_of(id).map(|(_, _, f)| f).unwrap_or(0);
                plants.entry((x, y, z)).or_insert(PlantEntry {
                    kind,
                    planted_tick: now,
                    stage: age,
                    facing,
                    fruit_done: false,
                });
            }
        }
        self.clear_timers(&stale);
    }

    /// P26：重启后把存量树苗重新纳入生长。
    /// redb 模式先按 `timers` 的原 planted 续算；无 timer 的存量苗仍从现在重计时。
    pub fn adopt_existing_saplings(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let restored = self.take_restored(|r| matches!(r, Restored::OakSapling));
        let mut stale = Vec::new();
        {
            let mut saplings = self.saplings.lock().unwrap_or_else(|p| p.into_inner());
            for rec in restored {
                if world.get(rec.pos.0, rec.pos.1, rec.pos.2) == mc_world::OAK_SAPLING_BLOCK_ID {
                    saplings.entry(rec.pos).or_insert(rec.planted_tick);
                } else {
                    stale.push((TimerKind::Sapling, rec.pos));
                }
            }
            for pos in world.sapling_blocks() {
                saplings.entry(pos).or_insert(now);
            }
        }
        self.clear_timers(&stale);
    }
}
