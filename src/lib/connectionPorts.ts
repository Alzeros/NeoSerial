/** 未连接时持续检测热插拔；若 MCP 先恢复连接而 GUI 端口列表尚为空，也继续补拉。 */
export function shouldPollPorts(connected: boolean, ports: readonly { port: string }[]): boolean {
  return !connected || ports.length === 0;
}

export function normalizePorts(ports: readonly { port: string; device: string | null }[]): { port: string; device: string | null }[] {
  const unique = new Map<string, { port: string; device: string | null }>();
  for (const entry of ports) {
    const trimmed = entry.port.trim();
    if (!trimmed) continue;
    const port = /^com\d+$/i.test(trimmed) ? trimmed.toUpperCase() : trimmed;
    const device = entry.device?.trim() || null;
    const existing = unique.get(port);
    if (!existing) unique.set(port, { port, device });
    else if (!existing.device && device) existing.device = device;
  }
  return [...unique.values()];
}
