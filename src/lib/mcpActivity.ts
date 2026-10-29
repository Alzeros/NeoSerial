export interface McpActivityCall {
  id: number;
  tool: string;
  port: string | null;
  started_at_ms: number;
  finished_at_ms: number | null;
  ok: boolean | null;
}

export interface McpActivitySnapshot {
  revision: number;
  active: McpActivityCall[];
  recent: McpActivityCall[];
}

export interface McpActivityState extends McpActivitySnapshot {
  running: boolean;
}

export function activityPort(connected: boolean, current: string | null, bound: string | null, selected: string): string | null {
  return (connected ? current : null) || bound || selected || null;
}

const labels: Record<string, [string, string]> = {
  list_ports: ['枚举串口', '枚举串口调用已完成'],
  connect: ['连接串口', '连接串口调用已完成'],
  disconnect: ['断开串口', '断开串口调用已完成'],
  get_status: ['查询串口状态', '状态查询已完成'],
  send: ['提交发送', '发送已提交'],
  send_and_read: ['发送并等待串口响应', '发送并读取调用已完成'],
  get_history_since: ['读取串口历史', '读取历史调用已完成'],
  reset_stats: ['清零串口统计', '清零统计调用已完成'],
  send_file: ['发送文件', '文件已写入驱动'],
  start_logging: ['启动连续存盘', '启动存盘调用已完成'],
  stop_logging: ['停止连续存盘', '停止存盘调用已完成'],
  sequence_run: ['启动序列', '启动序列调用已完成'],
  sequence_stop: ['停止序列', '停止序列调用已完成'],
  get_sequence_status: ['查询序列状态', '序列状态查询已完成'],
  get_settings: ['读取配置', '读取配置调用已完成'],
  save_settings: ['保存配置', '保存配置调用已完成'],
  kb_search: ['检索指令手册', '手册检索调用已完成'],
  kb_get: ['读取指令手册', '读取手册调用已完成'],
  kb_manuals: ['查询手册列表', '手册列表查询已完成'],
};

export function acceptActivitySnapshot(current: McpActivitySnapshot | null, incoming: McpActivitySnapshot): McpActivitySnapshot {
  return current && current.revision >= incoming.revision ? current : incoming;
}

function ago(timestamp: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - timestamp) / 1000));
  return seconds < 60 ? `${seconds} 秒前` : seconds < 3600 ? `${Math.floor(seconds / 60)} 分钟前` : `${Math.floor(seconds / 3600)} 小时前`;
}

export function activitySummary(snapshot: McpActivitySnapshot, port: string | null, now: number): { text: string; detail: string; tone: 'active' | 'idle' | 'error' } {
  const active = snapshot.active.filter(call => call.port === port).sort((first, second) => first.started_at_ms - second.started_at_ms);
  const scope = port ? `当前端口：${port}` : '应用级';
  const prefix = port ? '' : '应用级 · ';
  if (active.length) {
    const first = active[0];
    const elapsed = Math.max(0, Math.floor((now - first.started_at_ms) / 1000));
    const name = labels[first.tool]?.[0] ?? '工具调用';
    return {
      text: `${prefix}${active.length > 1 ? `${active.length} 项调用 · ` : ''}${name} · ${elapsed}s`,
      detail: `${scope}\n${active.map(call => labels[call.tool]?.[0] ?? '工具调用').join('、')}`,
      tone: 'active',
    };
  }
  const recent = snapshot.recent.find(call => call.port === port);
  if (recent) {
    const name = recent.ok ? labels[recent.tool]?.[1] ?? '工具调用已完成' : `${labels[recent.tool]?.[0] ?? '工具调用'}失败`;
    return {
      text: `${prefix}最近：${name} · ${ago(recent.finished_at_ms ?? recent.started_at_ms, now)}`,
      detail: `${scope}\n${name}`,
      tone: recent.ok ? 'idle' : 'error',
    };
  }
  return { text: `${prefix}暂无执行中的调用`, detail: scope, tone: 'idle' };
}
