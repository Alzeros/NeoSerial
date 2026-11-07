import { installFrontendDiagnostics, recordFrontend, registerFrontendState } from './lib/frontendDiagnostics';
import { getCurrentWebview } from '@tauri-apps/api/webview';

try { installFrontendDiagnostics(); }
catch (error) { recordFrontend('diagnostics.install_failed', { error }, 'error', true); console.warn('初始化前端诊断失败:', error); }
async function startFrontend(): Promise<void> {
  // Keep UI imports behind the diagnostic hooks so startup/module errors are captured too.
  const { mount } = await import('svelte');
  await import('./app.css');
  const target = document.getElementById('app');
  if (!target) throw new Error('应用挂载节点不存在');
  if (getCurrentWebview().label === 'settings') {
    const { default: SettingsDialog } = await import('./components/SettingsDialog.svelte');
    mount(SettingsDialog, { target, props: { standalone: true } });
  } else {
    const [{ default: App }] = await Promise.all([
      import('./App.svelte'),
      import('./lib/startup').then(({ preloadBeforeMount }) => preloadBeforeMount()),
    ]);
    mount(App, { target });
  }
  recordFrontend('frontend.mounted', {}, 'info', true);
  const stores = await import('./lib/stores.svelte');
  registerFrontendState('stores', stores);
}

void startFrontend().catch(error => {
  recordFrontend('frontend.start_failed', { error }, 'error', true);
  console.error('界面启动失败:', error);
});
