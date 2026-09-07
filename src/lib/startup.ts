import { getCurrentWebview } from '@tauri-apps/api/webview';
import { getSettings, loadSequenceAuto } from './tauri';
import { applySettings, scriptModules } from '$lib/stores';

/** 预取的兜底上限。IPC 正常几毫秒就回;万一卡住,到点先挂载(退回"先画默认值、设置到了再套"的老路),
 *  不让窗口一直空白。晚到的结果照常套上。 */
const PRELOAD_TIMEOUT_MS = 1000;

let settingsLoad: Promise<void> | null = null;

/** 加载持久化设置并回填 store(单飞:重复调用共享同一次 IPC)。失败只记日志,界面按默认值跑。 */
export function loadSettingsOnce(): Promise<void> {
  settingsLoad ??= getSettings()
    .then(applySettings)
    .catch((e) => console.error('加载设置失败:', e));
  return settingsLoad;
}

let sequenceLoad: Promise<string | null> | null = null;
let sequencePreloadTaken = false;

/** 从 sequence.json 加载快捷指令灌进 scriptModules(单飞)。
 *  resolve 为灌入后的 JSON 快照,ScriptSequencer 拿它当"与磁盘一致"的基准判断要不要自动保存;
 *  文件不存在/加载失败为 null——预置内容留在 store 里,随后被自动保存写盘。 */
export function loadSequenceOnce(): Promise<string | null> {
  sequenceLoad ??= loadSequenceAuto()
    .then((modules) => {
      if (modules.length === 0) return null;
      scriptModules.length = 0;
      scriptModules.push(...modules);
      return JSON.stringify(scriptModules);
    })
    .catch((e) => {
      console.error('自动加载序列配置失败:', e);
      return null;
    });
  return sequenceLoad;
}

/** ScriptSequencer 挂载时取预取结果:只有第一次挂载拿得到,之后返回 null。
 *  收起再展开右栏会重新挂载组件,这时预取快照已是旧的——收起期间组件的 sequence-changed
 *  监听已注销,其他窗口的改动本窗口没收到——拿到 null 的调用方必须现读磁盘,不能沿用内存里的内容。 */
export function takeSequencePreload(): Promise<string | null> | null {
  if (sequencePreloadTaken) return null;
  sequencePreloadTaken = true;
  return loadSequenceOnce();
}

/** 挂载前预取。Svelte 一挂载就画首帧,而设置(快捷指令密度/主题/日志字体)和快捷指令内容
 *  原先都是 onMount 之后才发 IPC 取回再套上去——首帧按代码里的默认值画,IPC 回来再重排一次,
 *  开窗口时能看见快捷指令区"刷"一下。这里先取回来灌进 store 再挂载,首帧直接就是用户的配置。
 *  get_settings 只 clone 内存里的一份、sequence.json 是个小文件,合计几毫秒,相对 WebView 起来
 *  到内容就位那一秒无感。
 *  主题编辑器窗口不预取:它自己加载设置并强制切到 custom 主题,先套一遍保存的主题反而多闪一次。 */
export async function preloadBeforeMount(): Promise<void> {
  if (getCurrentWebview().label === 'theme-editor') return;
  const loads = Promise.all([loadSettingsOnce(), loadSequenceOnce()]);
  await Promise.race([loads, new Promise((resolve) => setTimeout(resolve, PRELOAD_TIMEOUT_MS))]);
}
