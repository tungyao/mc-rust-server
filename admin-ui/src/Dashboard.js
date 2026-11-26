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
          <tr><th>conn</th><th>用户名</th><th>地址</th><th>位置</th><th>在线(s)</th><th>操作</th></tr>
        </thead>
        <tbody>
          <tr v-for="p in online" :key="p.conn_id">
            <td>{{ p.conn_id }}</td>
            <td>{{ p.username }}</td>
            <td>{{ p.addr }}</td>
            <td>{{ fmtPos(p.pos) }}</td>
            <td>{{ p.online_secs }}</td>
            <td><n-button size="tiny" type="error" secondary @click="kick(p)">踢人</n-button></td>
          </tr>
          <tr v-if="!online.length"><td colspan="6" class="muted">当前无在线玩家</td></tr>
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

    const userOptions = computed(() =>
      online.value.map(p => ({ label: p.username, value: p.username })));
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

    function fmtPos(pos) {
      return (pos || []).map(v => Number(v).toFixed(1)).join(',');
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
          username: giveUser.value,
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
        const r = await jpost('/api/kick', { username: p.username });
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
        invData.value = await jget('/api/inventory?username=' + encodeURIComponent(invUser.value));
      } catch (e) {
        invError.value = 'fetch failed: ' + e;
      }
    }

    const timers = [];
    onMounted(async () => {
      await Promise.all([loadItems(), refreshWorld(), refreshOnline()]);
      timers.push(setInterval(refreshWorld, 5000));
      timers.push(setInterval(refreshOnline, 2000));
    });
    onBeforeUnmount(() => timers.forEach(clearInterval));

    return {
      worldText, online, saving, saveMsg, fmtPos, kick, saveNow,
      userOptions, giveUser, giveCount, giveCat, giveQuery, categoryOptions,
      selectedItem, filteredItems, visibleItems, giveHint, giveMsg, giveItem,
      invUser, invCat, invQuery, invSlots, invEmptySlots, invHint, showInv,
    };
  },
};
