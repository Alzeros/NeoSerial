import { mount } from 'svelte';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import './app.css';
const target = document.getElementById('app')!;
if (getCurrentWebview().label === 'settings') {
  import('./components/SettingsDialog.svelte').then(({ default: SettingsDialog }) => {
    mount(SettingsDialog, { target, props: { standalone: true } });
  });
} else {
  Promise.all([
    import('./App.svelte'),
    import('./lib/startup').then(({ preloadBeforeMount }) => preloadBeforeMount()),
  ]).then(([{ default: App }]) => mount(App, { target }));
}
