import test from 'node:test';
import assert from 'node:assert/strict';
import {mcpEndpoint, mcpSetupGuide, MCP_CLIENTS} from '../src/lib/mcpSetup.ts';

test('endpoint uses actual IPv4 port and does not invent an address when stopped', () => {
  assert.equal(mcpEndpoint({running:true,port:34601}), 'http://127.0.0.1:34601/mcp');
  for (const port of [null, 0, -1, 65536, 3.5, NaN]) {
    assert.equal(mcpEndpoint({running:true,port}), null);
  }
  assert.equal(mcpEndpoint({running:false,port:34594}), null);
});

test('generic is the first option and needs no client-specific JSON wrapper', () => {
  assert.equal(MCP_CLIENTS[0].value, 'generic');
  const guide = mcpSetupGuide('generic','user','http://127.0.0.1:34600/mcp');
  assert.equal(guide.blocks.length, 0);
  assert.match(guide.instructions, /Streamable HTTP/);
});

test('Claude explicitly selects user or project-local scope and names the correct verification', () => {
  const url = 'http://127.0.0.1:34600/mcp';
  const user = mcpSetupGuide('claude','user',url);
  const project = mcpSetupGuide('claude','project',url);
  assert.equal(user.blocks[0].code, `claude mcp add --scope user --transport http neoserial ${url}`);
  assert.match(project.blocks[0].code, /--scope local/);
  assert.match(project.instructions, /项目/);
  assert.match(user.verification, /claude mcp get neoserial/);
  assert.match(user.notes, /同名/);
});

test('Codex has user CLI and TOML; project guide only edits project TOML', () => {
  const url = 'http://127.0.0.1:34600/mcp';
  const user = mcpSetupGuide('codex','user',url);
  const project = mcpSetupGuide('codex','project',url);
  assert.equal(user.blocks[0].code, `codex mcp add neoserial --url ${url}`);
  assert.equal(user.blocks[1].code, `[mcp_servers.neoserial]\nurl = "${url}"`);
  assert.equal(project.blocks.length, 1);
  assert.equal(project.blocks[0].code, user.blocks[1].code);
  assert.match(project.instructions, /\.codex\/config\.toml/);
  assert.match(project.notes, /信任/);
});

test('Cursor and VS Code use their respective JSON roots, with chosen scope instructions', () => {
  const url = 'http://127.0.0.1:34600/mcp';
  assert.deepEqual(JSON.parse(mcpSetupGuide('cursor','user',url).blocks[0].code),
    {mcpServers:{neoserial:{url}}});
  assert.deepEqual(JSON.parse(mcpSetupGuide('vscode','project',url).blocks[0].code),
    {servers:{neoserial:{type:'http',url}}});
  assert.match(mcpSetupGuide('cursor','project',url).instructions, /\.cursor\/mcp\.json/);
  assert.match(mcpSetupGuide('vscode','user',url).instructions, /MCP: Open User Configuration/);
});
