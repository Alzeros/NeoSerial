import { commandIndexLoad, onCommandIndexChanged, onSendHistoryChanged, sendHistoryLoad } from './tauri';
import type { ManualCommand, ManualDocument } from './types';

/** 指令联想的内存态:手册索引缓存(整份载入)+ 发送历史。
 *  真相都在后端(缓存文件 / AppState.send_history),这里只是镜像:启动时拉一次,之后靠广播事件更新。 */
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

/** App onMount 调:订阅两个广播事件,再做初次加载。返回清理函数。
 *  先 listen 再 load:后端启动自动刷新可能在 webview 订阅前就发出 command-index-changed,
 *  反过来的顺序会丢掉那次事件,窗口一直显示旧缓存直到下次刷新。 */
export function initCommandIndex(): () => void {
  const unlistenIndex = onCommandIndexChanged(() => {
    reloadCommandIndex().catch((e) => console.error('重载指令库缓存失败:', e));
  });
  const unlistenHistory = onSendHistoryChanged((items) => {
    commandIndex.history = items;
  });
  // listen() 返回 Promise,订阅真正生效在它 resolve 后;初次加载排在两者之后
  Promise.all([unlistenIndex, unlistenHistory])
    .then(() => Promise.all([
      reloadCommandIndex(),
      sendHistoryLoad().then((items) => (commandIndex.history = items)),
    ]))
    .catch((e) => console.error('加载指令库缓存/发送历史失败:', e));
  return () => {
    unlistenIndex.then((f) => f());
    unlistenHistory.then((f) => f());
  };
}
