// Feasibility check for the desktop "pause tracking" switch against real Codex.
// Scenario A: paused before the session. Scenario B: paused right after the
// Agent's first write, while its session is still running.
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync, readFileSync } from 'node:fs';
import { DatabaseSync } from 'node:sqlite';
import path from 'node:path';
import os from 'node:os';

const repo = path.resolve(import.meta.dirname, '../../..');
const mcp = path.join(repo, 'target/debug/agentkanban-mcp.exe').replaceAll('\\', '/');
const codexJs = path.join(process.env.APPDATA, 'npm/node_modules/@openai/codex/bin/codex.js');
const rule = readFileSync(path.join(repo, 'examples/codex-AGENTS-snippet.md'), 'utf8');
const root = path.join(os.tmpdir(), `ak-pause-eval-${Date.now()}`);
const out = path.join(import.meta.dirname, 'pause-runs');
mkdirSync(out, { recursive: true });
const prompt = '给 calc.js 增加 divide(a, b)，除数为 0 时抛出错误，并补上测试，最后跑通 npm test。';

function makeProject(name) {
  const dir = path.join(root, name);
  mkdirSync(dir, { recursive: true });
  writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name, private: true, type: 'module', scripts: { test: 'node --test' } }));
  writeFileSync(path.join(dir, 'calc.js'), 'export const add = (a, b) => a + b;\n');
  writeFileSync(path.join(dir, 'calc.test.js'), "import test from 'node:test';\nimport assert from 'node:assert/strict';\nimport { add } from './calc.js';\ntest('add', () => assert.equal(add(2, 3), 5));\n");
  writeFileSync(path.join(dir, 'AGENTS.md'), rule);
  for (const args of [['init', '-q'], ['add', '.'], ['-c', 'user.email=e@x', '-c', 'user.name=e', 'commit', '-q', '-m', 'init']]) execFileSync('git', args, { cwd: dir });
  return dir;
}

// Open the board the way the desktop app does (creates schema), then use SQL for the flag.
function openBoard(dataDir) {
  mkdirSync(dataDir, { recursive: true });
  const init = [{ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'e', version: '1' } } }];
  execFileSync(mcp, [], { input: init.map((m) => JSON.stringify(m)).join('\n') + '\n', env: { ...process.env, AGENTKANBAN_DATA_DIR: dataDir } });
  const db = new DatabaseSync(path.join(dataDir, 'agentkanban.sqlite3'));
  db.exec('PRAGMA busy_timeout=5000');
  return {
    setPaused: (paused) => db.prepare("INSERT INTO settings(key,value) VALUES ('tracking_paused',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(paused ? '1' : '0'),
    revision: () => db.prepare("SELECT value FROM metadata WHERE key='revision'").get().value,
    tasks: () => db.prepare('SELECT task_key,status,progress FROM tasks').all(),
    close: () => db.close(),
  };
}

function runCodex(label, project, dataDir, onPoll) {
  const args = [codexJs, 'exec', '--json', '--ephemeral', '--skip-git-repo-check', '-C', project, '-c', 'features.memories=false',
    '-c', `mcp_servers.agentkanban.command='${mcp}'`, '-c', `mcp_servers.agentkanban.env.AGENTKANBAN_DATA_DIR='${dataDir.replaceAll('\\', '/')}'`, '-'];
  return new Promise((resolve) => {
    const child = spawn(process.execPath, args, { cwd: project });
    let stdout = '';
    child.stdout.on('data', (d) => { stdout += d; });
    child.stdin.end(prompt);
    const poll = onPoll ? setInterval(onPoll, 200) : null;
    child.on('close', (code) => {
      if (poll) clearInterval(poll);
      writeFileSync(path.join(out, `${label}.jsonl`), stdout);
      const events = stdout.split('\n').filter(Boolean).flatMap((l) => { try { return [JSON.parse(l)]; } catch { return []; } });
      const calls = events.filter((e) => e.type === 'item.completed' && e.item?.type === 'mcp_tool_call' && e.item.server === 'agentkanban')
        .map((e) => ({ tool: e.item.tool, paused: JSON.stringify(e.item.result ?? '').includes('"paused":true') }));
      const commands = events.filter((e) => e.type === 'item.completed' && e.item?.type === 'command_execution').map((e) => e.item.command);
      const final = events.filter((e) => e.type === 'item.completed' && e.item?.type === 'agent_message').at(-1)?.item.text ?? '';
      const usage = events.find((e) => e.type === 'turn.completed')?.usage;
      resolve({ label, exit: code, calls, testsRan: commands.some((c) => c.includes('npm test')), final, usage });
    });
  });
}

const results = [];
{
  const dataDir = path.join(root, 'board-a');
  const board = openBoard(dataDir);
  board.setPaused(true);
  const r = await runCodex('A-paused-before', makeProject('a'), dataDir);
  r.tasks = board.tasks();
  const divide = readFileSync(path.join(root, 'a', 'calc.js'), 'utf8').includes('divide');
  r.workDone = divide;
  board.close();
  results.push(r);
}
{
  const dataDir = path.join(root, 'board-b');
  const board = openBoard(dataDir);
  board.setPaused(false);
  let pausedAt = null;
  const r = await runCodex('B-paused-mid-session', makeProject('b'), dataDir, () => {
    if (pausedAt === null && board.revision() > 0) { board.setPaused(true); pausedAt = board.revision(); }
  });
  r.pausedAfterRevision = pausedAt;
  r.finalRevision = board.revision();
  r.tasks = board.tasks();
  r.workDone = readFileSync(path.join(root, 'b', 'calc.js'), 'utf8').includes('divide');
  board.close();
  results.push(r);
}
writeFileSync(path.join(out, 'results.json'), JSON.stringify({ root, results }, null, 2));
for (const r of results) console.log(JSON.stringify({ ...r, final: r.final.slice(0, 200) }));
console.log(`ROOT ${root}`);
