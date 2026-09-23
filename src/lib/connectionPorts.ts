/** 未连接时持续检测热插拔；若 MCP 先恢复连接而 GUI 端口列表尚为空，也继续补拉。 */
export function shouldPollPorts(connected: boolean, ports: readonly string[]): boolean {
  return !connected || ports.length === 0;
}
