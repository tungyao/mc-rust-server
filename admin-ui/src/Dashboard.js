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
      <div v-if="saveMsg" class="muted">{{ saveMsg }}</div>
    </n-card>

    <n-card size="small" title="在线玩家（2s 轮询）">
      <n-table :bordered="true" :single-line="false" size="small">
        <thead>
          <tr><th>conn</th><th>用户名</th><th>UUID</th><th>地址</th><th>位置</th><th>在线(s)</th><th>模式</th><th>操作</th></tr>
        </thead>
        <tbody>
          <tr v-for="p in online" :key="p.conn_id">
            <td>{{ p.conn_id }}</td>
            <td>{{ p.username }}</td>
            <td :title="p.uuid || '—'"><code>{{ shortUuid(p) }}</code></td>
            <td>{{ p.addr }}</td>
            <td>{{ fmtPos(p.pos) }}</td>
            <td>{{ p.online_secs }}</td>
            <td>
              <n-select :value="p.gamemode || 'survival'" :options="gamemodeOptions" size="tiny"
                        style="width: 100px" :loading="modeBusy === p.conn_id"
                        @update:value="v => setMode(p, v)" />
            </td>
            <td><n-button size="tiny" type="error" secondary @click="kick(p)">踢人</n-button></td>
          </tr>
          <tr v-if="!online.length"><td colspan="8" class="muted">当前无在线玩家</td></tr>
        </tbody>
      </n-table>
      <template #footer><span class="muted">在线 {{ online.length }} 人；下方“发放 / 查看背包”的玩家列表取自这里。</span></template>
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

    <n-card size="small" title="工作台会话（P47，仅内存）">
      <pre class="kv">{{ craft9Text }}</pre>
      <template #footer><span class="muted">右键放置 crafting_table 后登记；离开/断线即丢，无 block entity 存档。</span></template>
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
      } catch (e) { /* 下次轮询重试 */ }
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

    async function refreshWorld() {
      try {
        world.value = await jget('/api/world');
      } catch (e) {
        world.value = { ok: false, error: String(e) };
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
      worldText, online, saving, saveMsg, fmtPos, shortUuid, kick, saveNow,
      userOptions, giveUser, giveCount, giveCat, giveQuery, categoryOptions,
      selectedItem, filteredItems, visibleItems, giveHint, giveMsg, giveItem,
      invUser, invCat, invQuery, invSlots, invEmptySlots, invHint, showInv,
      gamemodeOptions, modeBusy, setMode,
      toolTier, tierOptions, selectedTool, toolOptions, toolUser, toolEnchant,
      enchantOptions, toolMsg, toolBlocks, giveTool, applyToolEnchant,
      recipeCat, recipeCatOptions, recipeQuery, recipes, filteredRecipes,
      visibleRecipes, recipeHint, selectedRecipe, selectedRecipeDetail, craft9Text,
    };
  },
};
