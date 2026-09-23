// Real-agent evaluation for v0.4 automatic tracking. Runs Codex CLI against
// throwaway projects and an isolated board database; never touches user data.
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const repo = path.resolve(import.meta.dirname, '../../..');
const mcp = path.join(repo, 'target/debug/agentkanban-mcp.exe').replaceAll('\\', '/');
const codexJs = path.join(process.env.APPDATA, 'npm/node_modules/@openai/codex/bin/codex.js');
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const root = path.join(os.tmpdir(), `ak-agent-eval-${stamp}`);
const out = path.join(import.meta.dirname, 'runs', stamp);
mkdirSync(out, { recursive: true });

const rule = readFileSync(path.join(repo, 'examples/codex-AGENTS-snippet.md'), 'utf8');
function makeProject(name, withRule = false) {
  const dir = path.join(root, 'projects', name);
  mkdirSync(dir, { recursive: true });
  writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name, private: true, type: 'module', scripts: { test: 'node --test' } }, null, 2));
  writeFileSync(path.join(dir, 'calc.js'), 'export const add = (a, b) => a + b;\nexport const subtract = (a, b) => a - b;\n');
  writeFileSync(path.join(dir, 'calc.test.js'), "import test from 'node:test';\nimport assert from 'node:assert/strict';\nimport { add, subtract } from './calc.js';\n\ntest('add', () => assert.equal(add(2, 3), 5));\ntest('subtract', () => assert.equal(subtract(5, 3), 2));\n");
  writeFileSync(path.join(dir, 'README.md'), '# calc\n\nA tiny calculator module.\n');
  if (withRule) writeFileSync(path.join(dir, 'AGENTS.md'), rule);
  execFileSync('git', ['init', '-q'], { cwd: dir });
  execFileSync('git', ['-c', 'user.email=eval@example.com', '-c', 'user.name=eval', 'add', '.'], { cwd: dir });
  execFileSync('git', ['-c', 'user.email=eval@example.com', '-c', 'user.name=eval', 'commit', '-q', '-m', 'init'], { cwd: dir });
  return dir;
}

function runCodex(label, project, prompt, dataDir) {
  const args = [codexJs, 'exec', '--json', '--ephemeral', '--skip-git-repo-check', '-C', project,
    '-c', 'features.memories=false'];
  if (dataDir) {
    args.push('-c', `mcp_servers.agentkanban.command='${mcp}'`,
      '-c', `mcp_servers.agentkanban.env.AGENTKANBAN_DATA_DIR='${dataDir.replaceAll('\\', '/')}'`);
  }
  args.push('-');
  const started = Date.now();
  return new Promise((resolve) => {
    const child = spawn(process.execPath, args, { cwd: project, stdio: ['pipe', 'pipe', 'pipe'] });
    let stdout = '', stderr = '';
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.stdin.end(prompt);
    const timer = setTimeout(() => child.kill(), 15 * 60_000);
    child.on('close', (code) => {
      clearTimeout(timer);
      writeFileSync(path.join(out, `${label}.jsonl`), stdout);
      if (stderr.trim()) writeFileSync(path.join(out, `${label}.stderr.txt`), stderr);
      resolve(summarize(label, stdout, code, Date.now() - started, Boolean(dataDir)));
    });
  });
}

function summarize(label, stdout, code, ms, withBoard) {
  const events = stdout.split('\n').filter(Boolean).flatMap((line) => { try { return [JSON.parse(line)]; } catch { return []; } });
  const usage = { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 };
  const calls = [];
  let finalMessage = '';
  for (const event of events) {
    if (event.type === 'turn.completed' && event.usage) for (const key of Object.keys(usage)) usage[key] += event.usage[key] ?? 0;
    const item = event.item;
    if (event.type === 'item.completed' && item?.type === 'mcp_tool_call' && item.server === 'agentkanban') {
      calls.push({ tool: item.tool, status: item.status, argChars: JSON.stringify(item.arguments ?? {}).length,
        resultChars: JSON.stringify(item.result ?? item.error ?? null).length, arguments: item.arguments,
        error: item.error ?? (item.result?.isError ? item.result.content?.[0]?.text : undefined) });
    }
    if (event.type === 'item.completed' && item?.type === 'agent_message') finalMessage = item.text;
  }
  return { label, withBoard, exit: code, seconds: Math.round(ms / 1000), usage,
    boardCalls: calls.length,
    byTool: calls.reduce((acc, c) => ({ ...acc, [c.tool]: (acc[c.tool] ?? 0) + 1 }), {}),
    boardChars: calls.reduce((n, c) => n + c.argChars + c.resultChars, 0),
    calls, finalMessage, mentionsBoard: /看板|AgentKanban|kanban|task_upsert/i.test(finalMessage) };
}

function dumpBoard(dataDir) {
  const lines = [
    { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'eval', version: '1' } } },
    { jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'task_list', arguments: { include_done: true, include_archived: true, detail: true, limit: 100 } } },
  ].map((m) => JSON.stringify(m)).join('\n') + '\n';
  const output = execFileSync(mcp, [], { input: lines, env: { ...process.env, AGENTKANBAN_DATA_DIR: dataDir } }).toString();
  return JSON.parse(output.trim().split('\n')[1]).result.structuredContent.items;
}

const series = process.argv[2] ?? 'all';
const results = [];
const boardDir = path.join(root, 'board');
const record = (r) => { results.push(r); console.log(JSON.stringify({ label: r.label, exit: r.exit, s: r.seconds, usage: r.usage, byTool: r.byTool, boardChars: r.boardChars, mentionsBoard: r.mentionsBoard })); };

const prompts = {
  small: '给 calc.js 增加 divide(a, b)，除数为 0 时抛出错误，并补上测试，最后跑通 npm test。',
  readonly: '解释一下 calc.js 现在的结构，以及测试是怎么组织和运行的。',
  followup: '再给 calc.js 加一个 power(base, exp)，同样补测试并跑通。',
  optout: '把 README 的标题改成 Calc Toolkit。这个小改动不用记到看板。',
  multi: '在这个项目里做一个命令行待办工具 todo.js：支持 add、list、done 三个子命令，数据保存到 todos.json；为三个命令写测试并跑通 npm test，最后在 README 里补一段用法。',
};

if (series === 'all' || series === 'board') {
  const p1 = makeProject('calc-board', true);
  record(await runCodex('board-1-small', p1, prompts.small, boardDir));
  record(await runCodex('board-2-readonly', p1, prompts.readonly, boardDir));
  record(await runCodex('board-3-followup', p1, prompts.followup, boardDir));
  record(await runCodex('board-4-optout', p1, prompts.optout, boardDir));
  const p2 = makeProject('todo-board', true);
  record(await runCodex('board-5-multi', p2, prompts.multi, boardDir));
  const p3 = makeProject('calc-board-repeat', true);
  record(await runCodex('board-6-small-repeat', p3, prompts.small, boardDir));
}
if (series === 'all' || series === 'baseline') {
  record(await runCodex('base-1-small', makeProject('calc-base'), prompts.small, null));
  record(await runCodex('base-6-small-repeat', makeProject('calc-base-repeat'), prompts.small, null));
  record(await runCodex('base-5-multi', makeProject('todo-base'), prompts.multi, null));
}
const board = existsSync(boardDir) ? dumpBoard(boardDir) : [];
writeFileSync(path.join(out, 'results.json'), JSON.stringify({ root, mcp, results, board }, null, 2));
console.log(`RESULTS ${path.join(out, 'results.json')}`);
console.log(`BOARD ${board.length} tasks: ${board.map((t) => `${t.task_key}[${t.status}/${t.review_status}] steps=${t.steps.length}`).join(' | ')}`);
