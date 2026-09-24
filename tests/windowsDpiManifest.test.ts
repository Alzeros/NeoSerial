import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

test('Windows 构建在进程启动前声明 System DPI 并保留 Common Controls', () => {
  const manifest = readFileSync('src-tauri/windows-system-dpi.manifest', 'utf8');
  assert.match(manifest, /<dpiAware xmlns="http:\/\/schemas\.microsoft\.com\/SMI\/2005\/WindowsSettings">true<\/dpiAware>/);
  assert.match(manifest, /<dpiAwareness xmlns="http:\/\/schemas\.microsoft\.com\/SMI\/2016\/WindowsSettings">system<\/dpiAwareness>/);
  assert.match(manifest, /name="Microsoft\.Windows\.Common-Controls"/);
  assert.doesNotMatch(manifest, /PerMonitor|true\/PM|requireAdministrator/);
});

test('普通构建默认启用系统 DPI，GitHub 发布不关闭默认 feature', () => {
  const cargo = readFileSync('src-tauri/Cargo.toml', 'utf8');
  const build = readFileSync('src-tauri/build.rs', 'utf8');
  assert.match(cargo, /^system-dpi\s*=\s*\[\]/m);
  assert.match(cargo, /^default\s*=\s*\["system-dpi"\]/m);
  assert.match(build, /cfg!\(feature = "system-dpi"\)/);
  assert.match(build, /windows\.app_manifest\(include_str!\("windows-system-dpi\.manifest"\)\)/);
  const workflow = readFileSync('.github/workflows/release.yml', 'utf8');
  assert.doesNotMatch(workflow, /--no-default-features/);
});
