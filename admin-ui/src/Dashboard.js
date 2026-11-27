import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useMessage } from 'naive-ui';
import { ITEM_CAP, jget, jpost, matchQ } from './api.js';

const dashboardTemplate = `
  <div class="wrap">
    <div class="head">
      <h1>MC Admin 面板</h1>
      <n-tag type="warning" size="small" :bordered="false">只听本机 · 无鉴权 · 禁止暴露公网</n-tag>
      <span class="muted">26.3 / 777 · Vue 3 + Naive UI</span>
    </div>

    <n-card size="small" title="世界状态（5s 轮询）">
      <template #header-extra>
        <n-button size="tiny" :loading="saving" @click="saveNow">立即存档</n-button>
      </template>
      <pre class="kv">{{ worldText }}</pre>
      <div class="row">
        <span class="label">时间滑块</span>
        <n-slider v-model:value="timeTick" :min="0" :max="23999" :step="100" style="width: 220px" />
        <n-button size="tiny" @click="setCustomTime">应用时间</n-button>
        <span class="label">天气（待真机确认）</span>
        <n-select v-model:value="weatherPreset" :options="weatherOptions" size="tiny" style="width: 110px" />
        <n-button size="tiny" @click="setWeatherNow">应用天气</n-button>
      </div>
      <div class="row">
        <span class="label">广播队列</span><code>{{ world?.broadcast_queue ?? 0 }}</code>
        <span class="label">丢弃</span><code>{{ world?.broadcast_dropped ?? 0 }}</code>
        <span class="label">合并方块</span><code>{{ world?.merged_blocks ?? 0 }}</code>
        <span class="muted">A3 限流默认关：{{ world?.broadcast_throttle ? '开' : '关' }}</span>
      </div>
      <div v-if="saveMsg" class="muted">{{ saveMsg }}</div>
    </n-card>

    <n-card size="small" title="时间 / 天气（P54 A6，天气待真机确认）">
      <n-alert type="info" :show-icon="false">
        set_time 0x73（Long + Bool）已验证；天气 game_event 0x27 的 type 值未抓包确认，
        只做服务端状态 + 后台显示，<b>不发天气包</b>。夜间天空光由 v2 逐列实现。
      </n-alert>
      <div class="row">
        <span class="label">时刻 tick</span>
        <n-input-number v-model:value="timeTick" :min="0" :max="23999" size="small" style="width: 140px" />
        <n-button size="small" @click="setTimePreset('day')">设为白天</n-button>
        <n-button size="small" @click="setTimePreset('night')">设为夜晚</n-button>
        <n-button type="primary" size="small" @click="setCustomTime">自定义 tick</n-button>
        <span class="muted">{{ timeMsg }}</span>
      </div>
      <div class="row">
        <span class="label">天气</span>
        <n-select v-model:value="weatherPreset" :options="weatherOptions" size="small" style="width: 130px" />
        <span class="label">持续 tick</span>
        <n-input-number v-model:value="weatherDuration" :min="1" :max="23999" size="small" style="width: 140px" />
        <n-button size="small" @click="setWeatherNow">设置天气</n-button>
        <span class="muted">{{ weatherMsg }}</span>
      </div>
    </n-card>

    <n-card size="small" title="存档运维（P54 A5）">
      <div class="row">
        <span class="label">journal seq</span><code>{{ world?.journal_seq ?? 0 }}</code>
        <span class="label">保留条数</span><code>{{ world?.journal_kept ?? 0 }}</code>
        <span class="label">最老 seq</span><code>{{ world?.oldest_kept ?? 0 }}</code>
        <span class="label">字节</span><code>{{ world?.journal_bytes ?? 0 }}</code>
      </div>
      <div class="row">
        <span class="label">保留窗口</span>
        <n-input-number v-model:value="journalKeep" :min="1" size="small" style="width: 140px" />
        <n-button size="small" :loading="trimming" @click="trimNow">裁剪 journal</n-button>
        <span class="muted">{{ journalMsg }}</span>
      </div>
      <div class="row">
        <span class="label">A3 限流</span>
        <n-select v-model:value="throttleEnabled" :options="boolOptions" size="small" style="width: 90px" />
        <span class="label">队列上限</span>
        <n-input-number v-model:value="throttleMax" :min="1" :max="65536" size="small" style="width: 130px" />
        <n-button size="small" @click="applyThrottle">应用（改包序需真机确认）</n-button>
        <span class="muted">{{ throttleMsg }}</span>
      </div>
    </n-card>

    <n-card size="small" title="在线玩家（2s 轮询）">
      <n-table :bordered="true" :single-line="false" size="small">
        <thead>
          <tr><th>conn</th><th>用户名</th><th>UUID</th><th>地址</th><th>位置</th><th>在线(s)</th><th>经验</th><th>生命/饥饿/饱和</th><th>模式</th><th>操作</th></tr>
        </thead>
        <tbody>
          <tr v-for="p in online" :key="p.conn_id">
            <td>{{ p.conn_id }}</td>
            <td>{{ p.username }}</td>
            <td :title="p.uuid || '—'"><code>{{ shortUuid(p) }}</code></td>
            <td>{{ p.addr }}</td>
            <td>{{ fmtPos(p.pos) }}</td>
            <td>{{ p.online_secs }}</td>
            <td>{{ p.xp ?? 0 }}</td>
            <td>{{ fmtSurvival(p) }}</td>
            <td>
              <n-select :value="p.gamemode || 'survival'" :options="gamemodeOptions" size="tiny"
                        style="width: 100px" :loading="modeBusy === p.conn_id"
                        @update:value="v => setMode(p, v)" />
            </td>
            <td><n-button size="tiny" type="error" secondary @click="kick(p)">踢人</n-button></td>
          </tr>
          <tr v-if="!online.length"><td colspan="10" class="muted">当前无在线玩家</td></tr>
        </tbody>
      </n-table>
      <template #footer>
        <span class="muted">在线 {{ online.length }} 人；模式和附魔改后即落盘，重连后仍生效。下方发放/查看背包的玩家列表取自这里。</span>
      </template>
    </n-card>

    <n-card size="small" title="经验发放（P53）">
      <div class="row">
        <span class="label">玩家</span>
        <n-select v-model:value="xpUser" :options="userOptions" filterable clearable
                  placeholder="选择在线玩家" size="small" style="width: 220px" />
        <span class="label">经验值</span>
        <n-input-number v-model:value="xpAmount" :min="1" :max="2147483647" size="small" style="width: 160px" />
        <n-button type="primary" size="small" :disabled="!xpUser" @click="grantXp">发放经验</n-button>
        <span class="muted">{{ xpMsg }}</span>
      </div>
    </n-card>

    <n-card size="small" title="生存状态（P53，只读 + 调试）">
      <n-table :bordered="true" :single-line="false" size="small">
        <thead><tr><th>玩家</th><th>生命</th><th>饥饿</th><th>饱和</th><th>疲劳</th></tr></thead>
        <tbody>
          <tr v-for="p in online" :key="'surv-' + p.conn_id">
            <td>{{ p.username }}</td>
            <td>{{ p.health ?? 20 }}</td><td>{{ p.food ?? 20 }}</td>
            <td>{{ p.saturation ?? 5 }}</td><td>{{ fmtNum(p.exhaustion) }}</td>
          </tr>
          <tr v-if="!online.length"><td colspan="5" class="muted">当前无在线玩家</td></tr>
        </tbody>
      </n-table>
      <div class="row">
        <span class="label">调试</span>
        <n-select v-model:value="foodUser" :options="userOptions" filterable clearable
                  placeholder="选择在线玩家" size="small" style="width: 200px" />
        <span class="label">饥饿</span>
        <n-input-number v-model:value="foodValue" :min="0" :max="20" size="small" style="width: 120px" />
        <span class="label">饱和</span>
        <n-input-number v-model:value="satValue" :min="0" :max="5" :step="0.5" size="small" style="width: 120px" />
        <n-button size="small" :disabled="!foodUser" @click="setFood">设置</n-button>
        <span class="muted">{{ foodMsg }}</span>
      </div>
    </n-card>

    <n-card size="small" title="统计（P53，仅服务端本地累积）">
      <div class="row">
        <span class="label">玩家</span>
        <n-select v-model:value="statUser" :options="userOptions" filterable clearable
                  placeholder="选择在线玩家" size="small" style="width: 220px" />
        <span class="label">起</span><n-input v-model:value="statFrom" placeholder="YYYY-MM-DD" size="small" style="width: 140px" />
        <span class="label">止</span><n-input v-model:value="statTo" placeholder="YYYY-MM-DD" size="small" style="width: 140px" />
        <n-button size="small" :disabled="!statUser" @click="loadStats">查询</n-button>
        <span class="muted">{{ statMsg }}</span>
      </div>
      <n-table :bordered="true" :single-line="false" size="small">
        <thead><tr><th>统计项</th><th>stat_id</th><th>日期(天)</th><th>次数</th></tr></thead>
        <tbody>
          <tr v-for="r in statRows" :key="r.stat_id + '-' + r.day">
            <td>{{ r.stat }}</td><td>{{ r.stat_id }}</td><td>{{ r.day }}</td><td>{{ r.count }}</td>
          </tr>
          <tr v-if="!statRows.length"><td colspan="4" class="muted">尚无统计记录（不推送客户端）</td></tr>
        </tbody>
      </n-table>
    </n-card>

    <n-card size="small" title="发放物品">
      <div class="row">
        <span class="label">玩家</span>
        <n-select v-model:value="giveUser" :options="userOptions" filterable clearable
                  placeholder="选择在线玩家" size="small" style="width: 220px" />
        <span class="label">数量(1-640)</span>
        <n-input-number v-model:value="giveCount" :min="1" :max="640" size="small" style="width: 140px" />
      </div>
      <div class="row">
        <span class="label">分类</span>
        <n-select v-model:value="giveCat" :options="categoryOptions" size="small" style="width: 170px" />
        <span class="label">搜索</span>
        <n-input v-model:value="giveQuery" placeholder="中文名/注册名/id" clearable size="small" style="width: 240px" />
        <span class="muted">{{ giveHint }}</span>
      </div>
      <div class="item-grid">
        <div v-for="it in visibleItems" :key="it.id" class="item-cell"
             :class="{ selected: it.name === selectedItem }" @click="selectedItem = it.name">
          <div>{{ it.name_zh }}</div>
          <small>{{ it.name }} #{{ it.id }}</small>
        </div>
      </div>
      <div class="row">
        <span class="label">已选</span><code>{{ selectedItem }}</code>
        <n-button type="primary" size="small" :disabled="!giveUser" @click="giveItem">发放</n-button>
        <span class="muted">{{ giveMsg }}</span>
      </div>
    </n-card>

    <n-card size="small" title="查看背包">
      <div class="row">
        <span class="label">玩家</span>
        <n-select v-model:value="invUser" :options="userOptions" filterable clearable
                  placeholder="选择在线玩家" size="small" style="width: 220px" />
        <span class="label">分类</span>
        <n-select v-model:value="invCat" :options="categoryOptions" size="small" style="width: 170px" />
        <span class="label">关键词</span>
        <n-input v-model:value="invQuery" placeholder="中文名/注册名" clearable size="small" style="width: 200px" />
        <n-button size="small" :disabled="!invUser" @click="showInv">查询</n-button>
      </div>
      <div class="item-grid">
        <div v-for="s in invSlots" :key="'s' + s.slot" class="item-cell">
          <div>{{ s.name_zh }} ×{{ s.count }}</div>
          <small>[{{ s.slot }}] {{ s.name }}</small>
        </div>
        <div v-for="n in invEmptySlots" :key="'e' + n" class="item-cell empty">[{{ n }}] 空</div>
      </div>
      <template #footer><span class="muted">{{ invHint }}</span></template>
    </n-card>

    <n-card size="small" title="工具等级 / 精准采集 / 时运（P47）">
      <n-alert type="info" :show-icon="false">
        挖掘按手持镐档位判定（木/石/铜/铁/钻石/下界合金），档位不足仍破坏但零掉落；
        时运给矿石确定性额外掉落，精准采集掉方块自身物品（无对应物品则回退原掉落）。
        MVP：无耐久/挖掘速度；附魔为连接级服务端开关，不解析 Slot data component。
      </n-alert>
      <div class="row">
        <span class="label">档位</span>
        <n-select v-model:value="toolTier" :options="tierOptions" size="small" style="width: 130px" />
        <span class="label">镐子</span>
        <n-select v-model:value="selectedTool" :options="toolOptions" filterable size="small" style="width: 240px" />
        <span class="label">玩家</span>
        <n-select v-model:value="toolUser" :options="userOptions" filterable clearable
                  placeholder="选择在线玩家" size="small" style="width: 200px" />
        <span class="label">附魔</span>
        <n-select v-model:value="toolEnchant" :options="enchantOptions" size="small" style="width: 130px" />
        <n-button type="primary" size="small" :disabled="!toolUser || !selectedTool" @click="giveTool">发放</n-button>
        <n-button size="small" :disabled="!toolUser" @click="applyToolEnchant">设置附魔</n-button>
        <span class="muted">{{ toolMsg }}</span>
      </div>
      <n-table :bordered="true" :single-line="false" size="small">
        <thead><tr><th>方块</th><th>block_id</th><th>所需档位</th><th>掉落物品</th><th>时运</th></tr></thead>
        <tbody>
          <tr v-for="b in toolBlocks" :key="b.block_id">
            <td>{{ b.block }}</td><td>{{ b.block_id }}</td><td>{{ b.tier_name }}</td>
            <td>{{ b.item }} #{{ b.item_id }}</td><td>{{ b.fortune ? '是' : '否' }}</td>
          </tr>
        </tbody>
      </n-table>
    </n-card>

    <n-card size="small" title="配方浏览（P47，2×2 / 3×3 共用 283 条）">
      <div class="row">
        <span class="label">分类</span>
        <n-select v-model:value="recipeCat" :options="recipeCatOptions" size="small" style="width: 160px" />
        <span class="label">搜索</span>
        <n-input v-model:value="recipeQuery" placeholder="配方名/物品名/id" clearable size="small" style="width: 240px" />
        <span class="muted">{{ recipeHint }}</span>
      </div>
      <div class="item-grid">
        <div v-for="r in visibleRecipes" :key="r.name" class="item-cell" @click="selectedRecipe = r.name"
             :class="{ selected: r.name === selectedRecipe }">
          <div>{{ r.result.name }} ×{{ r.result.count }}</div>
          <small>{{ r.name }} · {{ r.category }}</small>
        </div>
      </div>
      <template #footer><span class="muted">{{ selectedRecipeDetail }}</span></template>
    </n-card>

    <n-card size="small" title="工作台会话（P54 A8 落盘 / P55 B6 GUI）">
      <n-alert type="warning" :show-icon="false">
        B6 GUI 骨架：菜单类型 12 取自 wiki 推导，<b>未抓包校准</b>。默认关；开启后才可能断线，请用真机 A/B 验证。
      </n-alert>
      <div class="row">
        <span class="label">落盘</span>
        <n-tag :type="craft9?.persist_enabled ? 'success' : 'default'" size="small" :bordered="false">
          {{ craft9?.persist_enabled ? '已启用' : '未启用' }}
        </n-tag>
        <span class="label">GUI 开关</span>
        <n-select v-model:value="guiEnabled" :options="boolOptions" size="small" style="width: 90px" />
        <n-button size="tiny" type="primary" secondary @click="applyGuiToggle">应用</n-button>
        <span class="label">打开 GUI</span>
        <n-select v-model:value="screenUser" :options="userOptions" filterable clearable
                  placeholder="选择在线玩家" size="small" style="width: 220px" />
        <n-button size="tiny" :disabled="!screenUser" @click="openGuiNow">打开 GUI</n-button>
        <span class="muted">{{ screenMsg }}</span>
      </div>
      <div class="row">
        <span class="label">清空会话</span>
        <n-select v-model:value="craftClearUser" :options="userOptions" filterable clearable
                  placeholder="选择会话 owner" size="small" style="width: 220px" />
        <n-button size="tiny" type="error" secondary :disabled="!craftClearUser" @click="clearCraft9">清空</n-button>
        <span class="muted">{{ craftMsg }}</span>
      </div>
      <pre class="kv">{{ craft9Text }}</pre>
      <template #footer><span class="muted">断线写 redb block_entities 表，重进 Join 恢复；不发 0x06，不改包流。快照含 persisted/owner/last_saved/screen_open。</span></template>
    </n-card>

    <n-card size="small" title="广播">
      <n-alert type="warning" :show-icon="false">
        未实现：777 system_chat 包体无抓包真值，不猜字节（见计划 §6.4）。接口返回 501。
      </n-alert>
    </n-card>
  </div>
`;

export const Dashboard = {
  template: dashboardTemplate,
  setup() {
    const message = useMessage();
    const world = ref(null);
    const worldText = computed(() => world.value === null
      ? 'loading…'
      : JSON.stringify(world.value, null, 2));
    const online = ref([]);
    const saving = ref(false);
    const saveMsg = ref('');

    const catalog = ref([]);
    const selectedItem = ref('minecraft:cobblestone');
    const giveUser = ref(null);
    const giveCount = ref(64);
    const giveCat = ref('');
    const giveQuery = ref('');
    const giveMsg = ref('');

    const invUser = ref(null);
    const invCat = ref('');
    const invQuery = ref('');
    const invData = ref(null);
    const invError = ref('');

    // P51：身份以稳定 UUID 为准；旧服条目没有 uuid 时回退用户名（兼容旧 API）。
    const playerId = p => (p && p.uuid) || (p && p.username) || '';
    const shortUuid = p => {
      const id = playerId(p);
      return id ? id.slice(0, 8) : '—';
    };
    const userOptions = computed(() =>
      online.value.map(p => ({
        label: p.uuid ? `${p.username} · ${shortUuid(p)}` : p.username,
        value: playerId(p),
      })));
    const categoryOptions = computed(() => {
      const cats = [...new Set(catalog.value.map(i => i.category))].sort();
      return [{ label: '全部', value: '' }, ...cats.map(c => ({ label: c, value: c }))];
    });

    const filteredItems = computed(() => catalog.value.filter(it =>
      (!giveCat.value || it.category === giveCat.value) && matchQ(it, giveQuery.value)));
    const visibleItems = computed(() => filteredItems.value.slice(0, ITEM_CAP));
    const giveHint = computed(() => {
      const n = filteredItems.value.length;
      return '共 ' + n + ' 项' + (n > ITEM_CAP ? '（只显示前' + ITEM_CAP + '，请加过滤条件）' : '');
    });

    const filteringInv = computed(() => !!(invCat.value || (invQuery.value || '').trim()));
    const invSlots = computed(() => {
      const d = invData.value;
      if (!d || !d.ok) return [];
      return (d.slots_detail || []).filter(s =>
        (!invCat.value || s.category === invCat.value) && matchQ(s, invQuery.value));
    });
    const invEmptySlots = computed(() => {
      const d = invData.value;
      if (!d || !d.ok || filteringInv.value) return [];
      const used = new Set((d.slots_detail || []).map(s => s.slot));
      const out = [];
      for (let i = 0; i < 46; i++) if (!used.has(i)) out.push(i);
      return out;
    });
    const invHint = computed(() => {
      if (invError.value) return invError.value;
      const d = invData.value;
      if (!d) return '尚未查询';
      if (!d.ok) return '查询失败：' + JSON.stringify(d);
      const total = (d.slots_detail || []).length;
      return '非空 ' + total + ' 格' + (filteringInv.value ? '（显示 ' + invSlots.value.length + '）' : '');
    });

    // P47：工具等级 / 附魔 / 配方浏览 / 工作台会话 / 游戏模式。
    const gamemodeOptions = [
      { label: '生存', value: 'survival' },
      { label: '创造', value: 'creative' },
    ];
    const enchantOptions = [
      { label: '无', value: 'none' },
      { label: '时运', value: 'fortune' },
      { label: '精准采集', value: 'silk_touch' },
    ];
    const modeBusy = ref(null);
    const toolCatalog = ref(null);
    const toolTier = ref(null);
    const selectedTool = ref('minecraft:wooden_pickaxe');
    const toolUser = ref(null);
    const toolEnchant = ref('none');
    const toolMsg = ref('');
    const recipes = ref([]);
    const recipeCat = ref('');
    const recipeQuery = ref('');
    const selectedRecipe = ref('');
    const craft9 = ref(null);

    // P54：时间/天气、存档运维、广播限流、工作台会话清空。
    const timeTick = ref(1000);
    const timeMsg = ref('');
    const weatherPreset = ref('clear');
    const weatherDuration = ref(120);
    const weatherMsg = ref('');
    const weatherOptions = [
      { label: '晴', value: 'clear' },
      { label: '雨', value: 'rain' },
      { label: '雷暴', value: 'thunder' },
    ];
    const boolOptions = [
      { label: '关', value: false },
      { label: '开', value: true },
    ];
    const journalKeep = ref(10000);
    const journalMsg = ref('');
    const trimming = ref(false);
    const throttleEnabled = ref(false);
    const throttleMax = ref(256);
    const throttleMsg = ref('');
    const craftClearUser = ref(null);
    const craftMsg = ref('');
    // P55 B6：GUI 运行时开关 + 后台触发打开（默认从 /api/craft9 同步）。
    const guiEnabled = ref(false);
    const screenUser = ref(null);
    const screenMsg = ref('');
    const worldSynced = ref(false);

    // P53：经验发放 / 生存调试 / 本地统计（在线玩家身份仍以稳定 UUID 为准）。
    const xpUser = ref(null);
    const xpAmount = ref(1);
    const xpMsg = ref('');
    const foodUser = ref(null);
    const foodValue = ref(20);
    const satValue = ref(5);
    const foodMsg = ref('');
    const statUser = ref(null);
    const statFrom = ref('');
    const statTo = ref('');
    const statRows = ref([]);
    const statMsg = ref('');

    const tierOptions = computed(() => (toolCatalog.value?.tiers || []).map(t =>
      ({ label: `${t.tier} · ${t.name}`, value: t.tier })));
    const toolOptions = computed(() => (toolCatalog.value?.tools || [])
      .filter(t => toolTier.value === null || t.tier === toolTier.value)
      .map(t => ({ label: `${t.name} (tier ${t.tier}, #${t.item_id})`, value: t.item })));
    const toolBlocks = computed(() => toolCatalog.value?.blocks || []);
    const recipeCatOptions = computed(() => {
      const cats = [...new Set(recipes.value.map(r => r.category))].sort();
      return [{ label: '全部', value: '' }, ...cats.map(c => ({ label: c, value: c }))];
    });
    const filteredRecipes = computed(() => recipes.value.filter(r => {
      if (recipeCat.value && r.category !== recipeCat.value) return false;
      const q = (recipeQuery.value || '').trim().toLowerCase();
      if (!q) return true;
      return r.name.toLowerCase().includes(q)
        || (r.result.name || '').toLowerCase().includes(q)
        || String(r.result.item_id) === q
        || (r.needs || []).some(n => (n.name || '').toLowerCase().includes(q));
    }));
    const visibleRecipes = computed(() => filteredRecipes.value.slice(0, 120));
    const recipeHint = computed(() => {
      const n = filteredRecipes.value.length;
      return `共 ${n} 条` + (n > 120 ? '（只显示前 120）' : '');
    });
    const selectedRecipeDetail = computed(() => {
      const r = recipes.value.find(x => x.name === selectedRecipe.value);
      if (!r) return '点击卡片查看输入/输出明细';
      const inputs = (r.needs || []).map(n => `${n.name} ×${n.count} (#${n.item_id})`).join(' + ');
      return `${r.name}：${inputs} → ${r.result.name} ×${r.result.count} (#${r.result.item_id})；槽 2×2/3×3`;
    });
    const craft9Text = computed(() => {
      const s = craft9.value;
      if (!s || !s.ok) return 'loading…';
      return JSON.stringify(s.sessions || [], null, 2);
    });

    function fmtPos(pos) {
      return (pos || []).map(v => Number(v).toFixed(1)).join(',');
    }
    function fmtNum(v) {
      return Number(v ?? 0).toFixed(2);
    }
    function fmtSurvival(p) {
      return `${p.health ?? 20} / ${p.food ?? 20} / ${fmtNum(p.saturation)}`;
    }

    async function setMode(p, mode) {
      modeBusy.value = p.conn_id;
      try {
        const r = await jpost('/api/gamemode', { uuid: playerId(p), gamemode: mode });
        if (r.status !== 200 || !r.body.ok) message.error('切换失败：' + JSON.stringify(r.body));
        else message.success(`${p.username} → ${mode}`);
      } catch (e) {
        message.error('gamemode failed: ' + e);
      } finally {
        modeBusy.value = null;
        await refreshOnline();
      }
    }
    async function loadTools() {
      try {
        toolCatalog.value = await jget('/api/tools');
        if (toolCatalog.value?.ok && !selectedTool.value) {
          selectedTool.value = toolCatalog.value.tools[0]?.item || null;
        }
      } catch (e) {
        toolMsg.value = 'tools load failed: ' + e;
      }
    }
    async function loadRecipes() {
      try {
        recipes.value = (await jget('/api/recipes')).recipes || [];
      } catch (e) {
        recipeQuery.value = 'recipes load failed: ' + e;
      }
    }
    async function loadCraft9() {
      try {
        craft9.value = await jget('/api/craft9');
        if (typeof craft9.value?.screen_open === 'boolean') {
          guiEnabled.value = craft9.value.screen_open;
        }
      } catch (e) { /* 下次轮询重试 */ }
    }
    async function applyGuiToggle() {
      try {
        const r = await jpost('/api/workbench-gui', { enabled: !!guiEnabled.value });
        screenMsg.value = r.body.ok ? `GUI ${r.body.enabled ? '开' : '关'}（menu #${r.body.menu_id}，未校准）`
          : JSON.stringify(r.body);
        if (!r.body.ok) message.error(screenMsg.value);
      } catch (e) { screenMsg.value = 'workbench-gui failed: ' + e; message.error(screenMsg.value); }
      await loadCraft9();
    }
    async function openGuiNow() {
      if (!screenUser.value) { message.warning('请选择在线玩家'); return; }
      try {
        const id = screenUser.value;
        const isUuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id);
        const r = await jpost('/api/screen-open', isUuid ? { uuid: id } : { username: id });
        screenMsg.value = r.body.ok ? `已打开 GUI（conn ${r.body.conn_id}，menu #${r.body.menu_id}）`
          : JSON.stringify(r.body);
        if (!r.body.ok) message.error(screenMsg.value);
        else message.success('已向玩家发送工作台 GUI');
      } catch (e) { screenMsg.value = 'screen-open failed: ' + e; message.error(screenMsg.value); }
      await loadCraft9();
    }
    async function setTimePreset(preset) {
      try {
        const r = await jpost('/api/time', { preset });
        timeMsg.value = r.body.ok ? `tick=${r.body.tick}` : 'HTTP ' + r.status + ' ' + JSON.stringify(r.body);
        if (r.status === 200 && r.body.ok) message.success('时间已设置');
        else message.error(timeMsg.value);
      } catch (e) { timeMsg.value = 'time failed: ' + e; message.error(timeMsg.value); }
      await refreshWorld();
    }
    async function setCustomTime() {
      try {
        const r = await jpost('/api/time', { tick: Number(timeTick.value) });
        timeMsg.value = 'HTTP ' + r.status + ' ' + JSON.stringify(r.body);
        if (r.status === 200 && r.body.ok) timeTick.value = r.body.tick;
        else message.error(timeMsg.value);
      } catch (e) { timeMsg.value = 'time failed: ' + e; message.error(timeMsg.value); }
      await refreshWorld();
    }
    async function setWeatherNow() {
      try {
        const r = await jpost('/api/weather', {
          weather: weatherPreset.value, duration_ticks: Number(weatherDuration.value),
        });
        weatherMsg.value = r.body.ok ? `已设 ${r.body.weather}（待真机确认，未发包）` : JSON.stringify(r.body);
        if (!r.body.ok) message.error(weatherMsg.value);
      } catch (e) { weatherMsg.value = 'weather failed: ' + e; message.error(weatherMsg.value); }
      await refreshWorld();
    }
    async function trimNow() {
      trimming.value = true;
      try {
        const r = await jpost('/api/journal-trim', { keep: Number(journalKeep.value) });
        journalMsg.value = r.body.ok ? `已裁剪 ${r.body.removed} 条` : JSON.stringify(r.body);
        if (!r.body.ok) message.error(journalMsg.value);
      } catch (e) { journalMsg.value = 'trim failed: ' + e; message.error(journalMsg.value); }
      finally { trimming.value = false; await refreshWorld(); }
    }
    async function applyThrottle() {
      try {
        const r = await jpost('/api/broadcast-throttle', {
          enabled: !!throttleEnabled.value, max_queue: Number(throttleMax.value),
        });
        throttleMsg.value = r.body.ok ? `限流 ${r.body.enabled ? '开' : '关'}` : JSON.stringify(r.body);
        if (!r.body.ok) message.error(throttleMsg.value);
      } catch (e) { throttleMsg.value = 'throttle failed: ' + e; message.error(throttleMsg.value); }
      await refreshWorld();
    }
    async function clearCraft9() {
      if (!craftClearUser.value) { message.warning('请选择会话 owner'); return; }
      try {
        const r = await jpost('/api/craft9-clear', { uuid: craftClearUser.value });
        craftMsg.value = r.body.ok ? `已清空 ${r.body.owner}` : JSON.stringify(r.body);
        if (!r.body.ok) message.error(craftMsg.value);
      } catch (e) { craftMsg.value = 'clear failed: ' + e; message.error(craftMsg.value); }
      await Promise.all([loadCraft9(), refreshWorld()]);
    }
    async function giveTool() {
      if (!toolUser.value || !selectedTool.value) {
        message.warning('请选择玩家与镐子');
        return;
      }
      try {
        const r = await jpost('/api/give', { uuid: toolUser.value, item: selectedTool.value, count: 1 });
        toolMsg.value = 'HTTP ' + r.status + ' ' + JSON.stringify(r.body);
        if (r.status === 200 && r.body.ok) message.success('已发放 ' + selectedTool.value);
        else message.error('发放失败：' + JSON.stringify(r.body));
      } catch (e) {
        toolMsg.value = 'give tool failed: ' + e;
        message.error(toolMsg.value);
      }
    }
    async function applyToolEnchant() {
      if (!toolUser.value) {
        message.warning('请选择在线玩家');
        return;
      }
      try {
        const r = await jpost('/api/tool-enchant', {
          uuid: toolUser.value,
          enchant: toolEnchant.value,
        });
        toolMsg.value = 'HTTP ' + r.status + ' ' + JSON.stringify(r.body);
        if (r.status === 200 && r.body.ok) message.success('附魔已设置');
        else message.error('设置失败：' + JSON.stringify(r.body));
      } catch (e) {
        toolMsg.value = 'tool-enchant failed: ' + e;
        message.error(toolMsg.value);
      }
    }

    async function grantXp() {
      if (!xpUser.value) { message.warning('请选择在线玩家'); return; }
      try {
        const r = await jpost('/api/xp', { uuid: xpUser.value, amount: Number(xpAmount.value) });
        xpMsg.value = 'HTTP ' + r.status + ' ' + JSON.stringify(r.body);
        if (r.status === 200 && r.body.ok) message.success('经验已发放，总经验 ' + r.body.total_xp);
        else message.error('发放失败：' + JSON.stringify(r.body));
      } catch (e) { xpMsg.value = 'xp failed: ' + e; message.error(xpMsg.value); }
      await refreshOnline();
    }
    async function setFood() {
      if (!foodUser.value) { message.warning('请选择在线玩家'); return; }
      try {
        const r = await jpost('/api/set-food', {
          uuid: foodUser.value, food: Number(foodValue.value), saturation: Number(satValue.value),
        });
        foodMsg.value = 'HTTP ' + r.status + ' ' + JSON.stringify(r.body);
        if (r.status === 200 && r.body.ok) message.success('生存状态已设置');
        else message.error('设置失败：' + JSON.stringify(r.body));
      } catch (e) { foodMsg.value = 'set-food failed: ' + e; message.error(foodMsg.value); }
      await refreshOnline();
    }
    async function loadStats() {
      if (!statUser.value) { message.warning('请选择玩家'); return; }
      let q = '/api/stats?uuid=' + encodeURIComponent(statUser.value);
      if (statFrom.value) q += '&from=' + encodeURIComponent(statFrom.value);
      if (statTo.value) q += '&to=' + encodeURIComponent(statTo.value);
      try {
        const r = await jget(q);
        statRows.value = r.ok ? (r.stats || []) : [];
        statMsg.value = r.ok ? `共 ${statRows.value.length} 项` : JSON.stringify(r);
      } catch (e) { statMsg.value = 'stats failed: ' + e; }
    }

    async function refreshWorld() {
      try {
        world.value = await jget('/api/world');
      } catch (e) {
        world.value = { ok: false, error: String(e) };
      }
      const w = world.value;
      if (w && w.ok && !worldSynced.value) {
        timeTick.value = w.world_time ?? 1000;
        weatherPreset.value = w.weather || 'clear';
        journalKeep.value = w.journal_keep || 10000;
        throttleEnabled.value = !!w.broadcast_throttle;
        throttleMax.value = w.broadcast_max_queue || 256;
        worldSynced.value = true;
      }
    }
    async function refreshOnline() {
      try {
        online.value = (await jget('/api/online')).online || [];
      } catch (e) { /* 下次轮询重试 */ }
    }
    async function loadItems() {
      try {
        catalog.value = (await jget('/api/items')).items || [];
      } catch (e) {
        giveMsg.value = 'items load failed: ' + e;
      }
    }
    async function saveNow() {
      saving.value = true;
      try {
        const r = await jpost('/api/save', {});
        saveMsg.value = 'HTTP ' + r.status + ' ' + JSON.stringify(r.body);
        if (r.status === 200 && r.body.ok) message.success('存档请求完成');
        else message.error('存档失败：' + JSON.stringify(r.body));
      } catch (e) {
        saveMsg.value = 'save failed: ' + e;
        message.error(saveMsg.value);
      } finally {
        saving.value = false;
        await refreshWorld();
      }
    }
    async function giveItem() {
      if (!giveUser.value) {
        message.warning('请先选择在线玩家');
        return;
      }
      try {
        const r = await jpost('/api/give', {
          uuid: giveUser.value,
          item: selectedItem.value,
          count: Number(giveCount.value),
        });
        giveMsg.value = 'HTTP ' + r.status + ' ' + JSON.stringify(r.body);
        if (r.status === 200 && r.body.ok) message.success('已发放 ' + selectedItem.value);
        else message.error('发放失败：' + JSON.stringify(r.body));
      } catch (e) {
        giveMsg.value = 'give failed: ' + e;
        message.error(giveMsg.value);
      }
    }
    async function kick(p) {
      if (!window.confirm('踢掉 ' + p.username + '？')) return;
      try {
        const r = await jpost('/api/kick', { uuid: playerId(p) });
        message.info('HTTP ' + r.status + ' ' + JSON.stringify(r.body));
        await refreshOnline();
      } catch (e) {
        message.error('kick failed: ' + e);
      }
    }
    async function showInv() {
      if (!invUser.value) {
        message.warning('请先选择在线玩家');
        return;
      }
      invError.value = '';
      invData.value = null;
      try {
        invData.value = await jget('/api/inventory?uuid=' + encodeURIComponent(invUser.value));
      } catch (e) {
        invError.value = 'fetch failed: ' + e;
      }
    }

    const timers = [];
    onMounted(async () => {
      await Promise.all([
        loadItems(), refreshWorld(), refreshOnline(),
        loadTools(), loadRecipes(), loadCraft9(),
      ]);
      timers.push(setInterval(refreshWorld, 5000));
      timers.push(setInterval(refreshOnline, 2000));
      timers.push(setInterval(loadCraft9, 2000));
    });
    onBeforeUnmount(() => timers.forEach(clearInterval));

    return {
      world, worldText, online, saving, saveMsg, fmtPos, fmtNum, fmtSurvival, shortUuid, kick, saveNow,
      userOptions, giveUser, giveCount, giveCat, giveQuery, categoryOptions,
      selectedItem, filteredItems, visibleItems, giveHint, giveMsg, giveItem,
      invUser, invCat, invQuery, invSlots, invEmptySlots, invHint, showInv,
      gamemodeOptions, modeBusy, setMode,
      toolTier, tierOptions, selectedTool, toolOptions, toolUser, toolEnchant,
      enchantOptions, toolMsg, toolBlocks, giveTool, applyToolEnchant,
      recipeCat, recipeCatOptions, recipeQuery, recipes, filteredRecipes,
      visibleRecipes, recipeHint, selectedRecipe, selectedRecipeDetail, craft9Text,
      timeTick, timeMsg, setTimePreset, setCustomTime,
      weatherPreset, weatherOptions, weatherDuration, weatherMsg, setWeatherNow,
      journalKeep, journalMsg, trimming, trimNow,
      throttleEnabled, throttleMax, boolOptions, throttleMsg, applyThrottle,
      craftClearUser, craftMsg, clearCraft9,
      guiEnabled, screenUser, screenMsg, applyGuiToggle, openGuiNow,
      xpUser, xpAmount, xpMsg, grantXp,
      foodUser, foodValue, satValue, foodMsg, setFood,
      statUser, statFrom, statTo, statRows, statMsg, loadStats,
    };
  },
};
