import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';

async function source(path: string): Promise<string> {
  return await readFile(path, 'utf8');
}

test('设置窗口的关闭监听获准执行最后的 destroy，且权限不扩展到串口窗口', async () => {
  const files = (await readdir('src-tauri/capabilities')).filter(file => file.endsWith('.json'));
  const capabilities = await Promise.all(files.map(async file =>
    JSON.parse(await source(`src-tauri/capabilities/${file}`)) as { windows: string[]; permissions: string[] }
  ));
  const destroyGrants = capabilities.filter(capability => capability.permissions.includes('core:window:allow-destroy'));
  assert.ok(destroyGrants.some(capability => capability.windows.includes('settings')),
    'Tauri onCloseRequested calls destroy after the handler allows closing');
  assert.ok(destroyGrants.every(capability => capability.windows.every(label => label === 'settings')),
    'serial window close guards must not be bypassed');
});

test('设置窗口使用固定单例并保存跨加载跳转请求', async () => {
  const rust = await source('src-tauri/src/lib.rs');
  const state = await source('src-tauri/src/state.rs');
  const capability = JSON.parse(await source('src-tauri/capabilities/default.json')) as { windows: string[] };

  assert.match(rust, /const SETTINGS_WINDOW_LABEL: &str = "settings"/);
  assert.match(rust, /get_webview_window\(SETTINGS_WINDOW_LABEL\)/);
  assert.match(rust, /pending_settings/);
  assert.match(rust, /WebviewWindowBuilder::new\([\s\S]*SETTINGS_WINDOW_LABEL/);
  assert.match(rust, /\.decorations\(false\)/);
  assert.match(state, /pub pending_settings: Mutex<Option<SettingsOpenRequest>>/);
  assert.ok(capability.windows.includes('settings'), 'settings window must be covered by Tauri capability');
});

test('设置窗口只渲染设置页，并保留独立窗口预览事件字段', async () => {
  const app = await source('src/App.svelte');
  const settings = await source('src/components/SettingsDialog.svelte');
  const tauri = await source('src/lib/tauri.ts');
  const titleBar = await source('src/components/TitleBar.svelte');

  assert.match(app, /isSettingsWindow/);
  assert.match(app, /<SettingsDialog standalone=\{true\} \/>/);
  assert.match(settings, /data-tauri-drag-region/);
  assert.match(settings, /emit\('settings-preview'/);
  for (const field of ['log_font_latin', 'log_font_cjk', 'log_dir_label', 'text_encoding']) {
    assert.match(tauri, new RegExp(field));
  }
  assert.doesNotMatch(titleBar, /import SettingsDialog/);
  assert.match(titleBar, /openSettingsWindow/);
});

test('设置窗口首次打开使用最小尺寸并居中到调用窗口', async () => {
  const rust = await source('src-tauri/src/lib.rs');
  const settings = await source('src/components/SettingsDialog.svelte');

  assert.match(rust, /webview_window: tauri::WebviewWindow/);
  assert.match(rust, /const SETTINGS_MIN_WIDTH: f64 = 680\.0/);
  assert.match(rust, /const SETTINGS_MIN_HEIGHT: f64 = 500\.0/);
  assert.match(rust, /\.inner_size\(SETTINGS_MIN_WIDTH, SETTINGS_MIN_HEIGHT\)/);
  assert.match(rust, /outer_position\(\)/);
  assert.match(rust, /outer_size\(\)/);
  assert.match(rust, /set_position\(/);
  assert.match(settings, /<svg width="11" height="11" viewBox="0 0 12 12"/);
  assert.match(settings, /<rect x="1\.5" y="1\.5" width="9" height="9"/);
});

test('settings reuse is allowed to hide its native window', async () => {
  const capability = JSON.parse(await readFile(new URL('../src-tauri/capabilities/default.json', import.meta.url), 'utf8'));
  assert.ok(capability.permissions.includes('core:window:allow-hide'));
});

test('settings entry does not eagerly import the serial application', async () => {
  const source = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /import App from/);
  assert.match(source, /import\('\.\/components\/SettingsDialog\.svelte'\)/);
});

test('settings shortcuts map the module to the native camelCase argument', async () => {
  const source = await readFile(new URL('../src/lib/tauri.ts', import.meta.url), 'utf8');
  const command = source.slice(source.indexOf("await invoke('open_settings_window'"), source.indexOf('export async function takePendingSettings'));
  assert.match(command, /extModule:\s*request\.ext_module/);
  assert.doesNotMatch(command, /ext_module:\s*request\.ext_module/);
});
