import { createApp } from 'vue';
import {
  NAlert, NButton, NCard, NConfigProvider, NInput, NInputNumber,
  NMessageProvider, NSelect, NTable, NTag, darkTheme,
} from 'naive-ui';
import { Dashboard } from './Dashboard.js';
import './style.css';

const App = {
  components: { Dashboard },
  setup() { return { darkTheme }; },
  template: `
    <n-config-provider :theme="darkTheme">
      <n-message-provider>
        <dashboard />
      </n-message-provider>
    </n-config-provider>
  `,
};

const app = createApp(App);
for (const [name, comp] of Object.entries({
  NConfigProvider, NMessageProvider, NCard, NTable, NButton,
  NInput, NInputNumber, NSelect, NTag, NAlert,
})) {
  app.component(name, comp);
}
app.mount('#app');
