export type McpClient = 'generic' | 'claude' | 'codex' | 'cursor' | 'vscode';
export type McpScope = 'user' | 'project';
export const MCP_CLIENTS: {value: McpClient; label: string}[] = [
  {value:'generic',label:'通用 MCP 客户端'},
  {value:'claude',label:'Claude Code'},
  {value:'codex',label:'Codex'},
  {value:'cursor',label:'Cursor'},
  {value:'vscode',label:'VS Code Copilot'},
];

export function mcpEndpoint(status: {running: boolean; port: number | null}): string | null {
  const port = status.port;
  return status.running && port !== null && Number.isInteger(port) && port > 0 && port <= 65535
    ? `http://127.0.0.1:${port}/mcp` : null;
}

interface SetupGuide {
  instructions: string;
  blocks: {label: string; code: string}[];
  verification: string;
  notes: string;
  docs: string;
}

/** 模板是客户端特有配置，不存在所有客户端通用的配置文件格式。
 * 官方依据与核对日期见 docs/mcp-onboarding-review.md（2026-09-30）。 */
export function mcpSetupGuide(client: McpClient, scope: McpScope, url: string): SetupGuide {
  const user = scope === 'user';
  switch (client) {
    case 'claude': return {
      instructions: user ? '在终端运行，注册到当前用户，供各项目使用。'
        : '先在终端进入目标项目目录，再运行。仅当前用户在该项目可用。',
      blocks: [{label:'终端命令', code:`claude mcp add --scope ${user ? 'user' : 'local'} --transport http neoserial ${url}`}],
      verification: '在目标项目的终端运行 claude mcp get neoserial；在 Claude 会话中用 /mcp 检查连接和工具。',
      notes: '同名项已存在时，add 不会更新旧地址。先用 get 核对范围；项目中的同名项会优先于用户配置。配置后新建会话验证。',
      docs:'https://code.claude.com/docs/en/mcp',
    };
    case 'codex': return {
      instructions: user ? '终端命令与手动编辑 ~/.codex/config.toml 二选一，作用于当前用户。'
        : '将配置合并到目标项目的 .codex/config.toml。',
      blocks: [
        ...(user ? [{label:'终端命令',code:`codex mcp add neoserial --url ${url}`}] : []),
        {label: user ? '或：config.toml 配置' : '.codex/config.toml 配置',code:`[mcp_servers.neoserial]\nurl = "${url}"`},
      ],
      verification: 'codex mcp get neoserial 可核对配置；在目标项目新建 Codex 会话，用 /mcp 检查实际连接。',
      notes: '已有 [mcp_servers.neoserial] 时更新该段，避免重复添加。项目配置需在信任项目后生效；自定义配置目录以客户端环境为准。',
      docs:'https://developers.openai.com/codex/mcp',
    };
    case 'cursor': return {
      instructions: `合并到 ${user ? '~/.cursor/mcp.json（当前用户，各项目可用）' : '目标项目的 .cursor/mcp.json'}。`,
      blocks:[{label:'mcp.json 配置',code:JSON.stringify({mcpServers:{neoserial:{url}}},null,2)}],
      verification:'在 Cursor 的 MCP 设置中检查 neoserial 状态与工具列表；必要时重启客户端。',
      notes:'将 neoserial 合并到已有 mcpServers 中，保留其他服务。已有同名项时更新它，并检查项目配置是否覆盖用户配置。',
      docs:'https://cursor.com/docs/mcp',
    };
    case 'vscode': return {
      instructions:user ? '在命令面板执行 MCP: Open User Configuration，打开当前用户配置文件。'
        : '将配置合并到目标工作区的 .vscode/mcp.json。',
      blocks:[{label:'mcp.json 配置',code:JSON.stringify({servers:{neoserial:{type:'http',url}}},null,2)}],
      verification:'执行 MCP: List Servers，选择 neoserial 检查或启动；在 Copilot 的工具列表中确认可用。',
      notes:'将 neoserial 合并到已有 servers 中，保留其他服务。用户配置按 VS Code 配置文件区分；远程工作区需核对服务运行位置。',
      docs:'https://code.visualstudio.com/docs/agent-customization/mcp-servers',
    };
    default: return {
      instructions:'在客户端的“添加 MCP 服务”中选择 Streamable HTTP，名称填 neoserial，URL 使用上方服务地址。',
      blocks:[],
      verification:'保存后，在客户端检查连接状态及工具列表是否出现 list_ports、get_status 等工具。',
      notes:'不同客户端的配置格式和作用范围不同。上方地址供本机客户端使用；WSL、容器或远程客户端需要先确认能访问此服务。',
      docs:'https://modelcontextprotocol.io/docs/develop/connect-local-servers',
    };
  }
}
