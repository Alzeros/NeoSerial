import { commandIndexLoad, onCommandIndexChanged, onSendHistoryChanged, sendHistoryLoad } from './tauri';
import type { ManualCommand, ManualDocument } from './types';

/** 指令联想的内存态:手册索引缓存(整份载入)+ 发送历史。
 *  真相都在后端(缓存文件 / AppState.send_history),这里只是镜像:用到时拉一次,之后靠广播事件更新。 */
export const commandIndex = $state<{
  documents: ManualDocument[];
  commands: ManualCommand[];
  /** 上次刷新时间(RFC 3339);null = 从未刷新 */
  fetchedAt: string | null;
  /** 最近发送在前 */
  history: string[];
}>({ documents: [], commands: [], fetchedAt: null, history: [] });

export async function reloadCommandIndex() {
  const c = await commandIndexLoad();
  commandIndex.documents = c.documents;
  commandIndex.commands = c.commands;
  commandIndex.fetchedAt = c.fetched_at;
}

/** 事件订阅生效的时点。索引加载要排在它后面:后端刷新发出的 command-index-changed
 *  若落在订阅生效前,这边读到的还是旧缓存,而且不会再被事件纠正。 */
let listenersReady: Promise<void> = Promise.resolve();
/** 手册索引的单飞加载;null = 这个窗口还没要过索引 */
let indexLoad: Promise<void> | null = null;

/** 按需载入手册索引(幂等、单飞)。
 *  整份缓存随手册数量涨(实测 245 KB),而关掉指令联想的窗口根本用不上,所以不在启动时无条件读,
 *  改由真正要用的地方来要:输入框联想(启用时)、指令查询面板、设置页的指令联想子页。
 *  失败不缓存结果,下一个调用方可以重试。 */
export function ensureCommandIndexLoaded(): Promise<void> {
  indexLoad ??= listenersReady.then(reloadCommandIndex).catch((e) => {
    indexLoad = null;
    console.error('加载指令库缓存失败:', e);
  });
  return indexLoad;
}

/** App onMount 调:订阅两个广播事件,并载入发送历史。返回清理函数。
 *  手册索引不在这儿载,见 ensureCommandIndexLoaded。发送历史照常载:它不属于指令联想
 *  (输入框发送就记,与联想开关无关),设置页也常显条数,而且只有几十条。 */
export function initCommandIndex(): () => void {
  const unlistenIndex = onCommandIndexChanged(() => {
    // 没要过索引的窗口(联想关着、也没开指令查询)不必因为别处刷新就把整份缓存读进来
    if (!indexLoad) return;
    reloadCommandIndex().catch((e) => console.error('重载指令库缓存失败:', e));
  });
  const unlistenHistory = onSendHistoryChanged((items) => {
    commandIndex.history = items;
  });
  // listen() 返回 Promise,订阅真正生效在它 resolve 后
  listenersReady = Promise.all([unlistenIndex, unlistenHistory]).then(() => undefined);
  listenersReady
    .then(sendHistoryLoad)
    .then((items) => (commandIndex.history = items))
    .catch((e) => console.error('加载发送历史失败:', e));
  return () => {
    unlistenIndex.then((f) => f());
    unlistenHistory.then((f) => f());
  };
}
