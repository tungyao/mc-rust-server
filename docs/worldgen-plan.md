# 地图生成器（worldgen）完善方案：调研与路线图

> 分支 `p42-worldgen`（基于 main `9085671`），owner=`subagent-worldgen`，日期 2026-11-26。
>
> **本轮范围：调研 + 方案设计，不写生产代码、不改协议、不跑 cargo。** 唯一交付物是本文件。
> 不修改 `tasks/`、`HANDOFF.md`、`MEMORY.md`，不修改任何 `.rs` 或已有文档。
>
> 版本锁定：`VERSION.md` → MC 26.3 / protocol 777 / data_version 5023 / `data/777/`；
> 升级到 26.4 时整体切换数据文件 + codegen，不保留兼容。

---

## 0. TL;DR

1. 当前生成器（`crates/mc-world/src/worldgen/{mod,noise,encode,tests}.rs`）在**确定性、出生点保护、稀疏存档兼容、编码正确性**上已完成得不错；P16 真机 r=10 / 441 块无压缩 496–558ms 的预算也达标。
2. 但地形是"3 层 value noise + 固定 58 基高"，没有侵蚀/河流/山脉；biome 每 chunk 单值；洞穴是 4×4×4 方块块；矿只有 coal/iron 的均匀 hash；水静态；树只有 oak；光照只发"从最低地表 section 向上全 15 级"——逐条对应 HANDOFF §5.6 的 6 条限制。
3. 建议按 **P1 地基与低风险视觉 → P2 结构性地貌/多 biome/光照 → P3 流体与结构** 推进；每一阶段保留生成器版本开关与回退点，**永不原地改变同一 seed 的旧地形语义**（会污染 `overrides` / `state-<seed>.redb`）。
4. 关键约束不在"能不能生成"，而在 **ChunkData 0x2E 的 section/biome/heightmap/light 编码**（多 biome 必须扩展 section 的 biome paletted container）、**ChunkCache 缓存的是已编码包体**（算法一变必须失效）、**rayon 并行要求生成纯函数**、以及 **出生点 (369,381) 默认 seed y=73 的固定回归**。
5. 可用数据已经很全：`data/777/blocks.json` 给出 1286 个方块的 `default_state_id`，`full/blocks.json` + `properties.json` 给出状态和属性元数据，`registry_00.bin` 给出 67 个 biome 网络 ID；**只有"非默认 state 的准确索引/语义"和"vanilla 多 biome 容器/非均匀光照的真机包体"需要抓包或 26.3 jar 反编译确认**。

---

## 1. 现状盘点

### 1.1 代码地图与当前算法

| 模块 | 位置 | 当前实现（关键常量） |
|---|---|---|
| 高度 | `worldgen/mod.rs:141-176` | 世界坐标 3 层 value noise：`scale=128/48/24`、`amp=20/10/6`、`BASE_HEIGHT=58`，round 后 clamp 到 `58..=94`。`value_noise_2d` 是 4-hash lattice + quintic smootherstep（`noise.rs:40-54`）。 |
| 出生保护 | `worldgen/mod.rs:51-56,153-176` | 半径 16 内把 `h<66` 的列按 smootherstep 抬到 `SPAWN_DRY_SURFACE_Y=66`，半径 4.5 内平台；只抬不降。固定 `SPAWN_BLOCK_X/Z=(369,381)`；`spawn_pos().1=height_at+1`（默认 seed=73）。 |
| 分层 | `worldgen/mod.rs:261-296` | `SEA_LEVEL=63`；`h>65` → grass(9)+3 dirt(10)+stone(1)；`h≤65` → 顶层 4 格 sand(121)+stone；`h<63` → 上面填 water(89)。`motion_heights` = 最高非空气（含水/树）+1-MIN_Y。 |
| 洞穴 | `worldgen/mod.rs:74-81,309-331,281` | 3D value noise（scale 24、threshold 0.63），按 **4×4×4 格**采样一次并整格挖空；只在 `MIN_Y+9..=h-9` 且只对 stone 生效 → 地表 8 格壳完整。 |
| 矿 | `worldgen/mod.rs:83-94,335-356` | 坐标 hash 预筛 1/8，再对选中方块投确定性 hash：coal(136) `y∈0..80` 约 1%，iron(134) `y∈-40..56` 约 0.8%；只替换 stone。 |
| 树 | `worldgen/mod.rs:67-72,358-482` | 候选扫描 `base-2..=base+17`，干燥+4 邻域高差≤1+出生 7 格外；forest 1/10、plains 1/40；oak_log(140) 干 4..6，oak_leaves(294) 去角 5×5 + 3×3 + 十字；只写落在本 chunk 的方块，跨 chunk 树冠连续。 |
| biome | `worldgen/mod.rs:58-65,190-236,485-498` | 每 chunk 4×4 采样水/沙占比：水≥7→ocean(36)、沙≥7→beach(3)、否则低频森林噪声（scale 96, threshold 0.55）→forest(22)/plains(41)；出生 chunk 强制非水域。所有 section 共用一个 `biome_id`。 |
| 编码 | `worldgen/encode.rs:32-87` | 24 section：uniform air 用带 biome 的空容器；uniform water/stone 用单值容器（含真实 `block_count`/`fluid_count`）；混合节走间接 palette（BPE 自适应，>256 direct）；heightmap = MOTION_BLOCKING（BPE 9、37 longs）；light 从最低地表所在 section 向上全 15 级。 |
| 缓存 | `crates/mc-network/src/chunk_cache.rs:18-172` | 缓存**已编码的 0x2E 包体** `Vec<u8>`（约 50KB/块），FIFO 1024；`bodies()` 用 rayon `par_iter` 并行生成+编码；绑定 `MutableWorld` 后走基底+override，`invalidate(pos)` 按块失效。 |
| 存档 | `HANDOFF §3「可变世界与存档」/ §5.4`、`mutable.rs:61-69,346-386,541-566` | `overrides-<seed>.dat` 或 `state-<seed>.redb` 只存"与基底不同的方块"；生成 chunk 时贴 override，只重算被触碰列的 `heights`/`motion_heights`。 |
| 测试 | `worldgen/tests.rs:88-657` | 20 个单测覆盖同 seed 一致、不同 seed 不同、高度≤3 梯度、4 seed 出生点、水沙分层、树跨 chunk、洞穴/矿、biome、编码往返、负/远坐标、默认 seed 出生 y=73 固定回归。 |

### 1.2 优点（不要推倒重来）

- **纯确定性、无第三方依赖、无全局状态**：`WorldGen` 是 `Copy` 的 `{seed}`，所有随机性来自 `hash01(seed,x,y,z,salt)`（`noise.rs:28-37`），负坐标/远坐标用 wrapping 乘法安全；同一 seed + 坐标永远出同一块，天然适配 rayon 与重连缓存。
- **高度场质量够用**：value noise 平滑、梯度有单测保证 ≤3，出生保护用 smootherstep 两端导数为 0，不会形成 1 格墙。
- **出生点安全网完整**：3×3 干燥草地、7 格内无树、脚下无洞穴，且有 4 个 seed 的回归测试锁住。
- **跨 chunk 连续性**：树按世界坐标扫描 ±2 并只写本 chunk 内方块；高度直接取世界坐标，chunk 边界无缝。
- **编码/高度图/计数正确**：`block_count`、`fluid_count`、单值/palette/direct 容器、MOTION_BLOCKING、BitSet 光照都按 777 实现并有逐字节往返测试；P16 真机可进世界。
- **稀疏存档语义干净**：基底确定性 → 磁盘只存差异；override 为空时与纯基底逐字节一致；seed 按文件名/redb 文件隔离。

### 1.3 具体缺陷（逐条对应 HANDOFF §5.6）

| HANDOFF §5.6 条目 | 现状缺陷 | 具体表现 / 代码位置 |
|---|---|---|
| 水静态 | 只有 `water=89`（level 0 source）填满 `h+1..=63`；无流向、水位、游泳物理；`fluid_count` 只识别 89 | 默认 seed 视距内几乎无水（高度均值 ~76，`h<63` 约 0.2%），"水景"要换 seed=42 / -987654321；`encode.rs:57-60` 的计数在引入其他 water state 后会失真。 |
| 洞穴方块化 | 4×4×4 整格同挖，形状是 4 格立方台阶；阈值 0.63 产生大片连通"海绵" | `cave_mask`/`cave_carved`（`mod.rs:309-331,504-509`）；玩家可挖到洞穴后视觉更明显。 |
| 每 chunk 单一 biome，边界硬切 | 16×16 全节共用 `biome_id`，草色/树叶色/水色在 chunk 边界硬切；river/birch_forest/snowy_plains/meadow 等 67 项注册表里的 biome 全未用；无雪原/河流/结构 | `chunk_biome`（`mod.rs:198-218`）+ `encode.rs:43,47,64` 把单值塞进每个 section；出生 chunk 有强制覆盖逻辑，边界可能二次硬切。 |
| 树最小实现 | 仅 oak 单一树种；固定 5×5 去角冠形；只有 4..6 随机干高；无 sapling/树叶 decay（decay 只存在于 P27 运行时树苗路径）、无灌木/草花 | `tree_for`/`write_tree`（`mod.rs:405-482`）；森林 1/10、平原 1/40 密度刻意保守。 |
| 沙滩硬编码 | `h≤65` 就顶层 4 格 sand，陡坡露石头/沙侧面；无 gravel/sandstone 过渡、无雪/冰/黏土 | `mod.rs:261-279` 的 `sandy` 判定；`layers_are...`/`sandy_and...` 单测把这个行为锁死。 |
| 矿物只有 coal/iron | 均匀 hash 替换 stone，无高度分带/biome 差异/矿脉团簇；没有 copper/gold/redstone/diamond，也没有矿洞光照 | `ore_at`（`mod.rs:335-356`）；`mutable.rs:61-69` 的 `is_terrain` 白名单也不含新矿物。 |

**HANDOFF §5.6 之外的两个额外缺口**（同样影响观感，建议一并纳入）：

- **天空光简化**：`encode.rs:78-87` 取本 chunk 最低地表 `min_h`，从 `(min_h-MIN_Y)/16` 起到 23 节全发 2048B 的 15 nibble；已点亮的 section 内洞穴偏亮，水下也是 15，树冠/山谷不投影。HANDOFF §5.6 洞穴条与 §3「区块数据」都注明为已知简化。
- **地形多样性不足**：3 层固定 scale/amplitude 只有"缓丘"一种形态；没有侵蚀/山脊/河流雕刻，默认 seed 视野内看不到水，玩家第一眼只有树林+丘陵。

---

## 2. 目标与约束

### 2.1 确定性

- 同一 `(seed, chunk_pos)` 必须生成逐字节一致的 `GeneratedChunk` 和 0x2E 包体；跨实例、跨线程、跨进程一致（`tests.rs:88-102,632-643` 已锁）。
- 生成必须是**纯函数**：禁止全局可变 RNG、禁止依赖 chunk 生成顺序、禁止从"邻居 chunk 已生成结果"读数据。`WorldGen{Copy}` + 局部栅格缓存可以；全局 `Mutex<HashMap<ChunkPos,_>>` 不可以（会破坏 rayon 竞速并引入顺序依赖）。
- 邻域计算（树 ±2、洞穴边界、光照边界、河流侵蚀半径）只能从 seed 直接采样，或由当前 chunk 内线程私有缓存推导。

### 2.2 与 `overrides` / `state-<seed>.redb` 兼容

- 基底只存"确定性地形"，磁盘只存差异（`HANDOFF §5.4` / `docs/world-storage-plan.md`）；`MutableWorld::with_store` 注入时会把"与基底相同"的条目自愈丢弃（`mutable.rs:106-118`）。**所以改变基底算法会让旧 override 的语义漂移**（旧档里"挖成空气"的格子在新基底可能是石头，反而被当成编辑；反之原本被剪掉的编辑无法恢复）。
- 策略：**生成器带版本号**（`meta.generator`，`world-storage-plan.md` Schema v1 已预留该字段），`v1=legacy` 恒定不动；`v2` 只对**新 seed / 显式选择**启用，旧 seed 继续 v1；版本不匹配时拒绝启动或按文档迁移，绝不静默改用新基底。
- `is_terrain` 白名单（`mutable.rs:61-69`）和 `column_heights` 的地形顶语义必须随新方块同步扩展（新矿、gravel、podzol、snow 等），否则编辑后高度图/光照起点会错。`is_replaceable` 只含 air/water（`mutable.rs:55-58`），water 引入新 state 后要同步。
- `MutableWorld::spawn_pos` 的"被 override 埋住则上抬"逻辑保持不变。

### 2.3 ChunkData 0x2E / 编码约束（来源：`play_chunk.rs:1-45,162-353`、`docs/packet-map-777.md` §4）

- 包 ID 固定 `0x2E`（`level_chunk_with_light`）；字段顺序：ChunkX/Z + Heightmaps + Data + BlockEntities + LightData。包 ID 曾错写 `0x27`（实为 Game Event），不要再回退。
- 24 个 section，`y=-64..319`；每节：`block_count` Short（非空气，含水）、`fluid_count` Short（water 数）、block states Paletted Container、**biomes Paletted Container**、LightData。
- Paletted Container：BPE u8 + palette VarInt 列表 + Data Array；**1.21.5+ 不发送 Data Array 长度**，由 BPE 推算；BPE=0 为单值（只跟一个 VarInt，无 data array）；BPE=max(4,ceil(log2(palette)))；>256 值走 direct BPE=15。
- 当前 biome 容器是 **BPE=0 单值**（`encode.rs:43/48/64` 传入单个 `biome_id`）；改 4×4×4 多 biome 必须新增"64 项 biome 容器"编码器——本轮**不改**，P2 要先取得主会话许可（见 §6 决策点 2）。
- Heightmap：只有 `MOTION_BLOCKING`（type=4），256 列 × BPE 9 × 37 longs；值 = 最高阻挡方块 (含 water/log/leaves) +1-MIN_Y。新方块加入后要重新定义"阻挡"口径并与 `mutable.rs::column_heights` 对齐。
- LightData 26.3 的 BitSet = **VarInt 字节数 + 小端 bit 字节**（不是 long 数组）；每个 mask 中的 section 跟 2048B 的 nibble 数组（两位/格）；没有数组的 section 客户端按 0。字符串/数组长度都必须用 VarInt。
- 文件规模约束：`src` 每文件 ≤2000 行（HANDOFF §3）；当前 `mod.rs` 563 / `noise.rs` 133 / `encode.rs` 88 / `tests.rs` 657 / `lib.rs` 554，新增能力应拆 `worldgen/{terrain,caves,ores,biome,features,light}.rs`，不要单文件膨胀。

### 2.4 ChunkCache 失效

- 缓存 key 只有 `ChunkPos`，内容绑定构造时的 seed（`chunk_cache.rs:72-100`）；值是被编码后的包体，不缓存 `GeneratedChunk`（24×4096×u16≈196KB/块）。
- 任何改变基底输出的改动（地形、biome、洞穴、矿、树、光照、编码）都必须让**所有已缓存包体失效**：靠"进程重启时 cache 为空"或显式 `ChunkCache` 重建/清空；不能在运行时热切生成器版本而沿用旧 cache。
- 编辑路径不变：`MutableWorld::set` → `ChunkCache::invalidate(pos)` → 下次取包体时走 `generate_chunk` 基底+override 重编（`chunk_cache.rs:24-27,155-159`）。
- 新算法若使得"边界方块依赖邻 chunk 的地形"（树、光照、河流），必须仍然能**单 chunk 独立生成**，否则缓存/并行都会出错。

### 2.5 性能预算

- 有效视距 `2..=10`（`chunk_view.rs::clamp_view_radius`），初始块数 `(2r+1)^2 = 25..441`；HANDOFF §3/§4 实测基线：r=10 **无压缩 496–558ms**、threshold=256 压缩 1916ms、重连缓存命中 72ms；r=4 保持旧行为。
- 验收目标（同一台真机）：无压缩 441 块 **≤700ms**（约 ≤1.3× 基线）、压缩 ≤2.2s（压缩耗时占大头，worldgen 改动影响有限）、重连命中仍 <100ms；单 chunk CPU 生成+编码 **≤~10–12ms（debug）**；包体 ≤~60KB（基线约 50KB）。
- 噪声采样估算（现行）：高度缓存 22×22×12 ≈ 5800 次 `hash01`；biome 采样约 2000；洞穴 4×4×38×8 ≈ 4860；矿预筛 ~2400；树 ~800，合计约 **1.5 万次 `hash01` + 2 万次整数 hash + 98k 方块写入/编码**。若洞穴改成逐方块 3 层 3D noise（~20k×8×3≈48 万次），会放大 ~30× —— **禁止**，必须用 2×2×2/4×4×4 栅格 + 线性插值或预计算格点。

### 2.6 其他硬约束

- 版本只支持 26.3/777；ID 必须从 `data/777/*.json` / codegen 查表拿，不写死新数字（现有 `registry_id`/`state_index_id` 模式见 `mc-world/src/lib.rs:71-116`）。
- 不改 `tasks/`、`HANDOFF.md`、`MEMORY.md`；不改协议包 ID 或线格式；本轮不跑 cargo。
- 出生点 (369,381) 固定，默认 seed 出生 y=73 有固定回归单测（`tests.rs:645-657`）。

---

## 3. 改进方案（分模块）

> 每项格式：**目标 / 具体算法 / 数据来源 / 对缓存与存档的影响 / 优先级与工作量 / 风险**。
> 所有新常量都按"先数据后代码"：`registry_id(mc_registry::BLOCKS, "minecraft:xxx")`，不手写 ID。

### 3.1 地形高度：侵蚀、山脊、河流

- **目标**：从"一种缓丘"变成有平原/丘陵/山地/河谷的地形；保留确定性、≤3 梯度、出生点。
- **算法**：
  1. 保留 `legacy_height_at`（v1）不动；v2 在其上叠加**领域扭曲**（对 x/z 加一层 scale≈256 的低频噪声偏移）和**山脊噪声** `ridge = 1 - |2*value_noise_2d(scale≈96)-1|`，用 `max` / 加权和抬高山地。
  2. **河流**：一条 scale≈384、threshold≈0.52 的 2D 噪声 + 距离场 `river = smoothstep(0.06, 0.0, |n-0.52|)`，把高度向 `SEA_LEVEL-2` 下切，并在河谷两岸留 2–4 格沙/gravel 过渡；河宽约 3–7 格，避免 1 格细线。
  3. **侵蚀**：对 3×3 邻域高度做一次轻量热侵蚀 `h' = h - k*(局部均值 - h)`（k≈0.3），或直接把高频 octave 幅度按低频坡度衰减；不得破坏相邻列 ≤3（用单测兜底）。
  4. **出生保护**：所有 delta 乘 `(1 - spawn_blend)`；半径 ≤4.5 完全用 v1 高度，4.5..16 smootherstep 过渡，河流在保护圈内权重 0。这样默认 seed 出生 y=73、3×3 干燥草地不变。
- **数据来源**：噪声全是自研（`noise.rs` 可加 `ridge_noise`/`domain_warp`，仍无依赖）；不需要新数据文件。高度带与 biome 的映射可用 `registry_00.bin` 里的 temperature/downfall（见 §3.9）。
- **缓存/存档**：高度变化 = 基底变化 → 新生成器版本 + 全 cache 失效；只对新 seed 开启。`MutableWorld` 的 `heights` 回退值来自新基底，旧档必须留 v1。
- **优先级/工作量**：P2 核心，**1.5–2 人日**（含梯度/出生回归测试）。
- **风险**：河流可能穿过出生圈或与水体不连通；用保护权重 + 截图验证；山脊噪声可能让相邻列 >3，需要后处理/clamp。

### 3.2 4×4×4 多 biome

- **目标**：消除 chunk 边界草色/水色硬切，启用 river/birch_forest/snowy_plains/meadow 等已有 biome ID。
- **算法（两小步）**：
  1. **先做 4×4 水平子格（16 个 biome/chunk）**：每个 4×4 列单元按世界坐标采样温度/湿度/高度，用 `biome_from_climate(temp, downfall, height)` 判 ocean/beach/river/forest/plains/snowy 等；同一列的 24 个 section 复用该值。section 的 biome 容器变 BPE≥4（16 项），每个 section 大约 +30–40B，全 chunk +~1KB。
  2. 若要洞穴 biome，再扩成真 4×4×4（64 项）：`y` 每 4 格一节，`y<-32` 用深暗/洞穴类 biome；代价是 64 项容器和非均匀 `fluid_count` 校验。
  3. 保留 `GeneratedChunk.biome_id` 为"主 biome"（取 16/64 项众数）供旧 API/测试兼容，新增 `biomes: [i32; 16]` 或 `[i32; 64]`。
- **数据来源**：biome 网络 ID 从 `data/777/vanilla_config/registry_00.bin` 解析（**67 项，数组下标即网络 ID**；已确认 plains=41, forest=22, beach=3, ocean=36, river=42, birch_forest=4, snowy_plains=47, meadow=33，详见 §3.9）；温度/降水/颜色在该文件的 biome NBT（`effects.temperature/downfall`、`grass_color/foliage_color/water_color`）里，客户端会按注册表自行着色，服务端只需发 ID。
- **缓存/存档**：包体变化 → 全 cache 失效 + 生成器版本升 v2；`GeneratedChunk` / `encode.rs` / `chunk_cache.rs` / `mutable.rs` / `tests.rs` 都要改（下一轮实施）；协议层需新增 64 项 biome 容器编码函数（**本轮不动**）。存档仍只存 override，biome 不入盘。
- **优先级/工作量**：P2，**2–3 人日**（含协议 encoder helper + 单测 + 真机验证）。
- **风险**：section biome 容器编码若与 vanilla 不一致会导致客户端 crash/错色；先抓 vanilla ChunkData 对照（见 §3.9）；出生 chunk 的强制 forest/plains 要保留或改成保护圈内 biome 锁定。

### 3.3 洞穴：noise caves / cheese / 隧道

- **目标**：从"4 格方块块"变成有连绵隧道 + 局部大洞室，且不破坏地表壳。
- **算法**：
  1. **隧道**：两条 2D 噪声 `n1,n2`（scale≈180/240）定义 `max(|n1|,|n2|) > 0.72` 的带状空腔，用 2D 栅格（每 2×2 列采样一次）插值到列，纵向按 y 正弦/低频噪声扰动形成起伏隧道。
  2. **cheese**：现有 3D value noise threshold 提高到 ~0.68，**采样格从 4×4×4 降到 2×2×2**（`CAVE_CELL=2` → 2000 次采样、16000 hash，仍可接受），或保留 4×4×4 但在格内做三线性插值消除方块边（需要逐方块判断，成本上升，慎用）。
  3. 只在 `y ≤ h-9 && y ≥ MIN_Y+9` 且目标为 stone 时挖；地表 8 格壳不变（`tests.rs:465-507` 已有断言）。出生圈下方可再禁挖 5 格壳，防"出生点掉洞"。
- **数据来源**：无新数据；沿用 `noise.rs` 的 `value_noise_3d` / `hash01`。
- **缓存/存档**：同 §3.2（基底变化 → 版本 + cache 失效）；挖空后旧 override（如在原石头处放置的方块）语义仍按绝对 ID 生效，但"与旧基底相同而被剪掉"的编辑无法恢复 → 旧档留 v1。
- **优先级/工作量**：P2，**1–1.5 人日**。
- **风险**：逐方块 3D noise 性能放大（§2.5）；隧道可能贯通水面 → 用高度下限 + 地表壳双保护。

### 3.4 矿物：按高度 / biome / 矿脉分布

- **目标**：coal/iron 分层更真实，补齐 copper/gold/redstone/diamond/lapis，并按 biome/深度调概率、出现团簇矿脉。
- **算法**：
  1. 用一张"矿物带"表（min_y / max_y / 每千分比 / 适用 stone|deepslate / biome 限制）：coal 高、copper 中低、iron 中、gold 深、redstone 深、lapis 深窄带、diamond 最深；`y < -16` 把 stone 换成 deepslate(33774) 及对应 deepslate ore。
  2. **矿脉**：3D 低频 hash 噪声（scale≈8..12）> 阈值才允许该区域出矿，区域内概率放大；同 seed 同坐标稳定，依然只替换 stone/deepslate。
  3. 保留现有"1/8 预筛 + 放大概率"技巧，避免全量 hash；单测统计每种矿在若干 chunk 的个数与 y 区间。
- **数据来源**：`data/777/blocks.json` 已有 `coal_ore=136 / iron_ore=134 / copper_ore=31147 / gold_ore=132 / redstone_ore=8552 / diamond_ore=6863 / emerald_ore=11315 / lapis_ore=662 / deepslate=33774 / deepslate_*_ore`；`full/blocks.json` 给出状态数（矿通常单态）。
- **缓存/存档**：基底变化 → v2 + cache 失效；**必须同步扩展 `mutable.rs::is_terrain`**（否则新矿不被视作地形顶，编辑后高度图错）；`drop` 表（掉落已有映射，需确认新矿掉落）属下一轮。
- **优先级/工作量**：P1（仅补矿种/高度带）**0.5–1 人日**；P2（矿脉+deepslate）再加 **0.5 人日**。
- **风险**：biome 限制在 P1 只能按 chunk 主 biome，P2 才能按子格 biome；不要因矿脉把洞穴挖空率算错。

### 3.5 水体：静态 vs 流动

- **目标**：先让"水看得见、岸自然"，再决定是否做流体模拟。
- **静态方案（P1/P2，推荐）**：
  - 保留 source water(89, level=0)；河道用 §3.1 的下切 + 岸边 2–4 格 gravel/sandstone(677)/sand(121) 过渡；水下放 seagrass(2370)/tall_seagrass(2372)/kelp(18419) 与 clay(8616) 斑块；冰封 biome 在 `h==SEA_LEVEL` 放 ice(8597)。
  - `fluid_count` 仍只统计 water source，零协议改动；`is_replaceable` 不变。
- **流动方案（P3，需单独决策）**：
  - `water` 有 16 个 state（89..104），`properties.json` 指明 `level: 0..15`（`enum_name=level`）；**source vs flowing 的准确 state 语义必须由 26.3 jar/capture 确认**；flowing 的 `fluid_count` 口径、MOTION_BLOCKING 取值、客户端水位渲染都要重测。
  - 流体 tick 要进入 50ms/20Hz 循环，且每次改动要广播 BlockUpdate（P21 hub）、`ChunkCache::invalidate`、存档 override/redb journal；影响面远大于 worldgen。
- **数据来源**：静态方案全部来自 `blocks.json`；流动方案需要 jar 反编译 / vanilla 抓包（见 §3.9）。
- **缓存/存档**：静态方案随基底版本升级；流动方案的运行时编辑走现有 override 路径，但会显著增加 journal 体积。
- **优先级/工作量**：静态 **0.5 人日**（P1）；流动 **3–5 人日**（P3）。
- **风险**：默认 seed 视距内没有水（§1.3）；若要"第一眼看到河"，需要在出生保护圈外、视距内放一条河——但会在保护圈边界形成硬过渡，见 §6 决策点 5。

### 3.6 树木 / 植被多样性

- **目标**：至少 3 种树 + 草花灌木 + 水下植被；跨 chunk 连续、出生圈干净。
- **算法**：
  1. 把 `tree_for` 的"按 biome 密度投骰"扩成"按 biome 选 species"：forest→oak/birch，plains→oak 少量，taiga/snowy→spruce，river/beach→少量 bush；树干高按 species 用独立 salt 的 hash01。
  2. `write_tree` 抽象成 `TreeSpec { trunk_id, leaves_id, canopy_fn, min_h,max_h }`；冠形仍用去角 5×5 / 3×3 / 十字，spruce 用逐层收缩圆锥；扫描范围按最大冠半径自动扩（oak 2、spruce 3 → `TREE_SCAN_MIN/MAX` 要随之调整）。
  3. 地表植被：草方块 + 高度 > SEA_LEVEL+2 + `hash01 < density` → short_grass(2363)/tall_grass(14771)/fern(2364)/poppy(3816)/dandelion(3813)/dead_bush(2365)，水下放 seagrass/kelp；**所有植被必须遵守 `SPAWN_TREE_CLEAR=7` 的扩展规则**（至少 4 格内不放任何高于地面的植物），且不写入 `heights`（`motion_heights` 只含树，不含草花）。
  4. 树叶默认 state=294（distance/persistent/waterlogged 默认态）视觉可接受；非默认 distance 需状态索引支持（§3.9）。
- **数据来源**：`blocks.json` 已有 birch_log=146 / birch_leaves=350 / spruce_log=143 / spruce_leaves=322 / jungle_log=149 / acacia_log=152 / dark_oak_log=158 / cherry_log=155 / mangrove_log=164 / 对应 sapling、short_grass/tall_grass/fern/dead_bush/bush、各类花、seagrass/kelp 等（完整表见 §3.9）。
- **缓存/存档**：基底变化 → 版本 + cache 失效；`is_terrain` 不含植物，`motion` 口径维持；`PLACEABLE_BLOCK_IDS` / `is_placeable` 若允许玩家种新树苗要另开任务。
- **优先级/工作量**：P1，**1–1.5 人日**（含多树种跨 chunk 测试）。
- **风险**：冠半径变大后扫描范围/高度缓存（当前 -3..18）要同步扩，否则跨 chunk 树叶被裁；出生点 4 格内测试目前只查 oak log/leaves，新树种要加断言。

### 3.7 结构（村庄 / 遗迹）可行性

- **结论**：**本轮明确不做，P3 也只建议做"单 chunk 小遗迹"**，完整村庄不划算。
- **完整村庄的硬骨头**：
  - 跨 chunk 足迹（vanilla 村庄按网格 512 格 + salt 放置），需要 deterministically 查询"附近结构候选"并在多个 chunk 各自生成自己部分，不能用"生成时随机掷骰"。
  - jigsaw / structure NBT、战利品表、床/门的多方块状态、村民生成（实体/AI）都不在 `data/777/*.json` 里；`full/blocks.json` 只有 `block_entity_types` 名字列表，没有 NBT schema。
  - block entity 需要新的 0x2E BlockEntities 数组编码（当前恒为 0）+ Tick/交互 + 存档；影响协议与持久层。
- **可行的最小版（P3 可选）**：单 chunk 内 5×5 石砖/圆石小遗迹 + 一个 chest(5544)/barrel(24100)（block entity NBT 需抓包确认）；坐标由 chunk 级 hash 决定，落在干燥、平坦、非出生圈的位置；不生成怪物/战利品随机。
- **数据来源**：方块 ID 来自 `blocks.json`；block entity 类型名来自 `full/blocks.json::block_entity_types`；NBT/战利品需 vanilla 抓包或 26.3 jar（§3.9）。
- **优先级/工作量**：小遗迹 **1.5–2 人日**（P3）；村庄 **≥5 人日**（不建议）。
- **风险**：结构方块属于基底，结构内箱子内容若靠 override 存会膨胀；建议结构本身进基底、容器内容进 redb 新表（又一轮设计）。

### 3.8 天空光 / 地下光照

- **目标**：洞穴不再"全亮 15"，水下不再 15，树冠/山谷有阴影，矿洞可放光源。
- **算法**：
  1. **逐列天空光**：对每列取最高不透明方块（water 不算完全遮光，按衰减 1–3 处理），从该 y+1 起 15，向下每格 -1；对 16×16 列做一次简单 BFS 水平传播（边界用 `WorldGen::height_at` 直接采邻列，保证 chunk 独立生成）。
  2. 输出按 section 的 2048B nibble 数组；只把含非零值的 section 放进 sky mask，**零数组节不要发**（省包体）。需要 `encode_sky_light_data` 之外的新 encoder（本轮不改）。
  3. **block light**：洞穴/遗迹放 glowstone(8686)/sea_lantern(14739)/lantern(24197)，用同样的 BFS 从 level 15 递减，填 block mask + block light 数组；无光源时 block 数组保持空（与现状兼容）。
  4. 出生圈、水面、树冠下的视觉效果必须真机截图对照。
- **数据来源**：方块亮度/不透明度在 `full/blocks.json` 每个 state 的 `luminance`/`opacity` 字段；光照线格式在 `play_chunk.rs:306-353` 注释（wiki + 26.3 jar 反编译）；非均匀光照的 vanilla 真机包体建议抓一份对照（§3.9）。
- **缓存/存档**：光照是基底派生数据；算法变化 → v2 + cache 失效；`MutableWorld` 编辑后目前**不重算光照**（只重算高度），新方案仍需明确"编辑只局部更新光照 or 整块重算"，否则挖掘后洞穴依旧亮（`HANDOFF §5.6` 已记录该缺口）。
- **优先级/工作量**：P2 天空光 **1.5–2 人日**；P3 block light **1 人日**。
- **风险**：包体可能从 ~50KB 涨到 ~80KB（若所有 section 都发数组）；BFS 边界不一致会造成 chunk 接缝亮线；必须有"接缝截图"验收。

### 3.9 数据来源总表（哪些能查、哪些要抓包）

| 需要的数据 | 来源 | 状态 |
|---|---|---|
| 方块 `default_state_id`（air=0, stone=1, grass=9, dirt=10, water=89, sand=121, coal=136, iron=134, oak_log=140, oak_leaves=294 等 1286 项） | `data/777/blocks.json` | ✅ 已有 |
| 新方块 ID：coarse_dirt=11, podzol=13, sandstone=677, gravel=127, clay=8616, copper_ore=31147, gold_ore=132, redstone_ore=8552, diamond_ore=6863, emerald_ore=11315, lapis_ore=662, deepslate=33774, birch_log=146/leaves=350, spruce_log=143/leaves=322, jungle_log=149, acacia_log=152, dark_oak_log=158, cherry_log=155, mangrove_log=164, oak_sapling=30, birch_sapling=34, spruce_sapling=32, short_grass=2363, tall_grass=14771, fern=2364, large_fern=14773, dead_bush=2365, bush=2366, dandelion=3813, poppy=3816, cornflower=3825, lily_pad=10654, seagrass=2370, tall_seagrass=2372, kelp=18419, cactus=8599, sugar_cane=8617, snow=8589, snow_block=8598, ice=8597, packed_ice=14761, moss_block=33712, glowstone=8686, sea_lantern=14739, lantern=24197, chest=5544, barrel=24100, spawner=5444, stone_bricks=9488, bedrock=88 | `data/777/blocks.json`（应通过 `registry_id()` 查表，不硬编码） | ✅ 已有 |
| 非默认 blockstate 的属性与枚举值：`water.level=0..15`（16 态 89..104）、`oak_leaves.distance=1..7 & persistent & waterlogged`（28 态）、`oak_log.axis=x/y/z`（3 态）、`grass_block.snowy`、`wheat.age=0..7` 等 | `data/777/properties.json`（hash→名字/类型/取值）+ `data/777/full/blocks.json` 的 `states[]`（连续 ID 数组） | ⚠️ 元数据有；**state 索引与语义（source vs flowing、轴序、distance 序）需 26.3 jar / vanilla 抓包确认**，现有 codegen 只生成 `(first_state_id,name,count)` |
| 方块亮度/不透明度/碰撞/声音 | `data/777/full/blocks.json` 每 state 的 `luminance`/`opacity`/`state_flags`/`collision_shapes` | ✅ 已有 |
| biome 网络 ID（67 项，下标即 ID）：beach=3, birch_forest=4, ocean=36, forest=22, plains=41, river=42, meadow=33, snowy_plains=47, snowy_taiga=49, taiga=57, desert=15, swamp=54, warm_ocean=60 … | `data/777/vanilla_config/registry_00.bin`（按 `config.rs::decode_config_registry_data` 解析；现有常量只对 4 项开放，见 `play_chunk.rs:41-44`，HANDOFF §3 表格列出若干未用 ID） | ✅ 已有，需补常量 |
| biome 气候/颜色（temperature/downfall/has_precipitation/grass_color/foliage_color/water_color） | `registry_00.bin` 每项的 NBT `effects`/`attributes` | ✅ 已有 |
| 4×4×4 多 biome 的 vanilla 包体（biome Paletted Container BPE/palette 顺序） | wiki Chunk_format + **建议抓 vanilla 26.3 ChunkData 对照** | ❌ 需抓包/额外数据 |
| 非均匀 sky/block light 的 vanilla 包体与衰减规则 | `play_chunk.rs:306-353` 注释（wiki + 26.3 jar 反编译）；`full/blocks.json` 的 luminance/opacity | ⚠️ 格式有，实践需抓包对照 |
| 结构 jigsaw/loot table/block entity NBT schema | 不在 `data/777` JSON 中；需 26.3 jar / 原版结构包抓取 | ❌ 需额外数据（P3 才需要） |
| 版本/包 ID 对照 | `VERSION.md`、`docs/packet-map-777.md` | ✅ 已有 |

---

## 4. 分阶段路线图

### P1 — 地基与低风险视觉（推荐先做）

**做什么**

1. 引入生成器版本字段（`v1=legacy` 冻结、默认仍 v1；`v2` 只对新 seed/显式开关启用），明确"算法变更 → cache 全失效 + 存档版本校验"的流程（对应 §2.2/§2.4）。
2. 不改高度场：补矿种/高度带（§3.4 前半）、地表材质过渡（gravel/sandstone/snow/ice/coarse_dirt，§1.3 沙滩条）、树种与草花（§3.6）、静态水岸植被（§3.5 静态方案）。
3. 同步更新 `mutable.rs::is_terrain` / `is_replaceable` / `column_heights` 口径；单测补新方块的白名单与出生圈新树种断言。
4. 拆分 `worldgen` 子模块（`features.rs`/`ores.rs`），保持每文件 ≤2000 行。

**验证**

- `cargo test --workspace` 全绿（重点 `worldgen/tests.rs`、`chunk_cache.rs`、`play_stream.rs`、`mutable.rs`）；
- 新增单测：同 seed 逐字节一致、默认 seed 出生 y=73 与 3×3 干燥、新矿 y 区间/数量、多树种跨 chunk 连续、新方块被 `is_terrain` 正确识别；
- 真机：默认 seed / 42 / -987654321 各截 1 张出生视野图；`play join chunks sent ... count=441 elapsed_ms` 与基线 496–558ms 对比（≤700ms）。

**回退点**：生成器开关回 v1；P1 不动 biome 容器和光照编码，回退只涉及 `mc-world` 内的世界生成与 `mutable` 白名单，协议/存档格式零变化。

### P2 — 结构性地貌与多 biome

**做什么**

1. 地形侵蚀/山脊/河流（§3.1），河流在出生保护圈内权重 0；
2. 洞穴升级为 2×2×2/插值 + cheese/隧道（§3.3），矿脉与 deepslate（§3.4 后半）；
3. 4×4（先水平）/4×4×4 多 biome（§3.2）+ river/snowy/birch biome 启用；
4. 逐列天空光（§3.8 前半）。

**前置**：主会话批准"在 mc-protocol 增加 biome 容器/非均匀 light 编码 helper"（线格式不变，但属于协议层代码）；先抓 vanilla 包体对照。

**验证**

- 单测：河流连续性与不穿出生圈、洞穴仍保 8 格壳、biome 容器逐字节解码（沿用 `tests.rs::parse_sections` 扩展）、光照 nibble 在地表/水下/洞穴的值、种子一致性、远坐标；
- 真机：river/snowy/biome 边界与洞穴光照截图（含 chunk 接缝）；441 块无压缩 ≤700ms、包体 ≤60KB。

**回退点**：生成器版本回 v1（包体恢复旧编码）；新增 encoder helper 保留但不用；旧 seed 不迁 v2。若真机包体解码失败，优先回退 biome/light 编码两小项，不回退地形。

### P3 — 流体与结构（可选，需单独立项）

**做什么**：流体 state + 20Hz tick/广播/存档（§3.5 流动方案）、block light（§3.8 后半）、单 chunk 小遗迹（§3.7）；可选进一步做村庄。

**验证**：端到端（挖开放水、水流、重连/重启后 fluid 状态）、真机两人同屏广播、性能与 journal 体积统计。

**回退点**：P3 各能力独立开关；流体 tick 可整块移除而回到静态水。

### 本轮明确不做及理由

- **不改任何 `.rs`、不改协议、不跑 cargo**：本轮是调研/方案交付，用户要求只产出文档。
- **不改 `tasks/` / `HANDOFF.md` / `MEMORY.md`**：由主会话维护。
- **不做流体模拟**：影响面从 worldgen 扩到 tick、广播、存档、客户端渲染，收益/风险比差；先用静态河湖验证观感。
- **不做村庄/完整结构**：缺 jigsaw/loot/block entity NBT 数据，且跨 chunk 生成与存档设计成本高（§3.7）。
- **不换 seed、不改出生坐标、不破坏默认 seed y=73**：有固定回归测试与既有存档。
- **不做 Anvil 兼容**：`VERSION.md` / `docs/world-storage-plan.md` 明确 self-owned redb 格式，世界地形不入盘。
- **不引入第三方依赖**：`noise.rs` 的无依赖 hash/value noise 已够用；加 `noise` crate 会破坏"无依赖"约束。

---

## 5. 性能与风险

### 5.1 噪声采样与耗时估算

| 步骤 | 当前采样量/块 | 改进后（P2 估计） | 备注 |
|---|---|---|---|
| 高度缓存 22×22 | 484 列 × 3 octave × 4 hash ≈ 5800 | 加 domain warp/侵蚀 3×3 → ~2–3×，约 15k | 仍远小于洞穴；建议把高度缓存结果按列复用，不重复采样 |
| biome 采样 | chunk 16 次 + 邻 chunk 缓存 ~9×16 ≈ 2000 | 16 个子格 × 2–4 噪声 ≈ 100，反而更省 | 4×4×4 时 64 格仍 <500 |
| 洞穴 | 4×4×38 × 8 hash ≈ 4860 | 2×2×38 × 8 ≈ 1200（格更细、量更省）；逐方块 3D 则 ~48 万（禁止） | 用"低频格 + 插值"而非逐点 |
| 矿 | 1/8 预筛 ≈ 2400 hash + ~20k 整数 hash | 加矿脉噪声（3D 低频，按格缓存）≈ +几百 | 保持预筛 |
| 树/植被 | 400 候选 ×2 hash ≈ 800 | 多物种 + 草花 ~2–3× ≈ 2500 | 扫描范围只按最大冠半径扩 |
| 写入/编码 | 24×4096 方块 + palette | 多 biome：每节 +~40B；非均匀光：最多 +49KB，靠"零节不发"压回 | 包体目标 ≤60KB |

按 HANDOFF 实测 441 块 496–558ms 反推：单块 CPU 约 9–11ms（debug，rayon 多核摊薄）。**预算线：单块 CPU ≤12ms**；压缩路径 1916ms 主要耗在 zlib（level 6），worldgen 改动不是主瓶颈。

### 5.2 与 rayon 并行生成的兼容

- `ChunkCache::bodies()` 用 `positions.par_iter().map(|&pos| self.body(pos))`；`body()` 在锁外生成。生成函数必须 `Send + Sync` 且无共享可变状态。
- 邻域采样只能"按 seed 直接算"，不能"生成邻居 chunk 并读它的结果"：否则会递归、顺序依赖、缓存 key 失效。
- 线程私有缓存（如 biome 气候查表）可以，但必须是 `thread_local!` 或函数局部；不要引入跨 chunk 的全局 `HashMap`。
- 新编码函数不能依赖生成顺序；palette 必须按确定性顺序收集（当前 `encode_block_section_with_fluid` 已按首次出现顺序，保持）。

### 5.3 seed 变化 / 存档影响

- **同 seed 改算法**是最危险的情况：旧 override 是按旧基底剪裁过的稀疏差，换基底后"玩家挖掉的方块"会变成"与基底不同的新方块"被错误保留，而"玩家放置且与新基底相同"的方块会失去差异被剪掉。
- 防线：① v1 冻结；② `state-<seed>.redb::meta.generator` 写版本，启动校验；③ 版本不匹配时新 seed 新文件（旧文件保留 `.bak`），旧 seed 继续 v1；④ 文档写清"不做自动迁移"。
- **换 seed** 本身已有文件名隔离（`overrides-<seed>.dat` / `state-<seed>.redb`），风险低；但默认 seed 的出生 y 固定回归不可动。

### 5.4 出生点 (369,381) 保护

1. `legacy_height_at` 永久保留；v2 所有高度 delta 乘 `(1-spawn_blend)`，半径 ≤4.5 完全等于 v1 → 默认 seed 出生 y=73 不变，3×3 干燥草地。
2. 河流/侵蚀/雪冰/水下植被在保护圈内权重 0；树（所有树种）、草花沿用并扩大 `SPAWN_TREE_CLEAR=7`，保证 4 格内无遮挡。
3. 洞穴新算法仍只挖 `y≤h-9`，地表 8 格壳 + 出生圈下方可选再禁挖，防掉洞。
4. 回归测试：现有 4-seed 出生测试（`tests.rs:209-265`）、梯度测试（`123-184`）、默认 seed 固定回归（`645-657`）必须对新 profile 再跑一遍；新增"河道不进入保护圈"断言与真机出生截图。

### 5.5 其他风险

- **协议回归**：biome 容器 / light 编码器写错会导致客户端断线或停在"加载地形中"（HANDOFF §3 记录过 0x2E 错值与 Batch/GameEvent 的必要条件）；必须逐字节往返测试 + 真机。
- **包体膨胀**：非均匀光照最多 +49KB/块 × 441 ≈ +21MB 原始帧；必须跳过零 light 节，并在压缩阈值开启时复测 1916ms 基线。
- **编辑后光照不重算**：HANDOFF §5.6 已有缺口，P2 只解决"生成时"光照；玩家挖掘后的局部重算需单独设计（P3）。
- **性能回归隐蔽**：压缩路径的总耗时被 zlib 主导，单看 Join 总时长可能掩盖 worldgen 变慢；建议加一个"仅生成+编码"的 bench/单测计时输出。

---

## 6. 需要主会话 / 用户决策的点

1. **生成器版本策略**：推荐"v1 冻结为默认，v2 仅新 seed 开启"，代价是老玩家看不到新地形；是否接受？（若要求所有 seed 原地升级，必须先设计存档迁移。）
2. **协议层边界**：是否允许下一轮在 `mc-protocol` 增加"64 项 biome 容器 / 非均匀 light"编码 helper？线格式不变，但要改协议 crate 代码，超出本轮范围。
3. **水的目标**：只做静态河湖/沙滩（P1–P2，风险低），还是要做流动水位 + 游泳物理（P3，牵动 tick/广播/存档）？
4. **结构范围**：完全不做 / 单 chunk 小遗迹 / 完整村庄（≥5 人日且需额外数据）？推荐先做小遗迹或直接跳过。
5. **默认 seed 的"第一眼水景"**：当前 seed=123456789 视距内无水。是否要求在出生圈外、视距内强制有一条河/湖？（会引入出生保护边界的过渡与额外回归测试。）
6. **性能红线**：是否接受"视觉显著增强但无压缩 Join 劣化到 ~700ms（基线 1.3×）"？若红线更严（≤600ms），需要缩减洞穴/光照改动。
7. **数据来源投入**：非默认 state 语义、多 biome 容器、非均匀光照是否先抓一份 vanilla 26.3 ChunkData / 反编译 jar 再实施？推荐抓，避免协议层返工。
