import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, rm, rmdir, rename, writeFile, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createInterface } from 'node:readline';

// Black-box checks: only the public stdio protocol is used to change tasks.
// Every process receives an isolated data directory; user data is never opened.
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
let executable = path.join(repo, 'target', 'release', process.platform === 'win32' ? 'agentkanban-mcp.exe' : 'agentkanban-mcp');
let reportPath;
let keep = false;
let executableSet = false;
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === '--report') {
    assert.ok(args[i + 1], '--report requires a file path');
    reportPath = path.resolve(args[++i]);
  } else if (args[i] === '--keep') {
    keep = true;
  } else if (args[i] === '--help') {
    console.log('Usage: node scripts/verify-mcp.mjs [executable] [--report file.json] [--keep]');
    process.exit(0);
  } else if (!executableSet && !args[i].startsWith('--')) {
    executable = path.resolve(args[i]);
    executableSet = true;
  } else {
    throw new Error(`Unknown argument: ${args[i]}`);
  }
}
await access(executable);
const fixtureRoot = await mkdtemp(path.join(tmpdir(), 'agentkanban-mcp-check-'));
const dataDir = path.join(fixtureRoot, 'data');
const projectPath = path.join(fixtureRoot, '中文项目');
const concurrentProject = path.join(fixtureRoot, 'concurrent-project');
await Promise.all([dataDir, projectPath, concurrentProject].map((p) => mkdir(p, { recursive: true })));
const allClients = new Set();
const results = [];
const report = {
  started_at: new Date().toISOString(),
  executable,
  node: process.version,
  platform: process.platform,
  fixture_root: fixtureRoot,
  data_dir: dataDir,
  protocol_version: '2025-11-25',
  results,
};

class McpClient {
  constructor(directory = dataDir) {
    this.nextId = 1;
    this.pending = new Map();
    this.unmatched = [];
    this.outputErrors = [];
    this.stderr = '';
    this.closed = false;
    this.process = spawn(executable, [], {
      env: { ...process.env, AGENTKANBAN_DATA_DIR: directory },
      windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    allClients.add(this);
    this.process.stdin.on('error', () => {});
    this.process.stderr.setEncoding('utf8');
    this.process.stderr.on('data', (chunk) => { this.stderr = (this.stderr + chunk).slice(-16000); });
    this.lines = createInterface({ input: this.process.stdout, crlfDelay: Infinity });
    this.lines.on('line', (line) => {
      if (!line.trim()) return;
      let message;
      try {
        message = JSON.parse(line);
        assert.equal(message.jsonrpc, '2.0', 'Every stdout message must use JSON-RPC 2.0');
      } catch (error) {
        this.outputErrors.push(`Non-protocol stdout: ${line.slice(0, 300)}`);
        this.rejectPending(error);
        return;
      }
      const waiter = this.pending.get(message.id);
      if (waiter) {
        this.pending.delete(message.id);
        clearTimeout(waiter.timer);
        waiter.resolve(message);
      } else {
        this.unmatched.push(message);
      }
    });
    this.exit = new Promise((resolve) => {
      this.process.on('error', (error) => {
        this.closed = true;
        this.rejectPending(error);
        resolve({ error: error.message });
      });
      this.process.on('close', (code, signal) => {
        this.closed = true;
        this.rejectPending(new Error(`MCP exited (${code ?? signal}): ${this.stderr.trim()}`));
        resolve({ code, signal });
      });
    });
  }

  rejectPending(error) {
    for (const waiter of this.pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.pending.clear();
  }

  send(message) {
    assert.ok(!this.closed, `MCP process is closed: ${this.stderr}`);
    this.process.stdin.write(`${JSON.stringify(message)}\n`);
  }

  async rpc(method, params = {}) {
    const id = this.nextId++;
    assert.ok(!this.closed, `MCP process is closed: ${this.stderr}`);
    const response = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Timed out waiting for ${method}: ${this.stderr.trim()}`));
      }, 20000);
      this.pending.set(id, { resolve, reject, timer });
    });
    this.send({ jsonrpc: '2.0', id, method, params });
    return response;
  }

  async initialize(protocolVersion = '2025-11-25') {
    const response = await this.rpc('initialize', {
      protocolVersion,
      capabilities: {},
      clientInfo: { name: 'agentkanban-black-box-check', version: '0.1.0' },
    });
    assert.ok(!response.error, JSON.stringify(response.error));
    assert.equal(response.result.protocolVersion, protocolVersion);
    assert.ok(response.result.serverInfo?.name);
    assert.ok(response.result.capabilities?.tools);
    this.send({ jsonrpc: '2.0', method: 'notifications/initialized' });
    return response.result;
  }

  async call(name, arguments_ = {}) {
    const response = await this.rpc('tools/call', { name, arguments: arguments_ });
    assert.ok(!response.error, JSON.stringify(response.error));
    assert.ok(!response.result?.isError, JSON.stringify(response.result));
    const result = response.result;
    assert.ok(Array.isArray(result?.content), 'Tools return MCP content blocks');
    const textBlock = result.content.find((block) => block.type === 'text');
    assert.ok(textBlock, 'Tools include a text representation for older clients');
    const payload = JSON.parse(textBlock.text);
    if (result.structuredContent !== undefined) assert.deepEqual(result.structuredContent, payload);
    return payload;
  }

  async expectToolError(name, arguments_) {
    const response = await this.rpc('tools/call', { name, arguments: arguments_ });
    assert.ok(response.error || response.result?.isError, 'Expected an explicit tool or protocol error');
    const detail = response.error?.message ?? response.result?.content?.find((block) => block.type === 'text')?.text;
    assert.ok(typeof detail === 'string' && detail.trim().length > 0, 'Errors must explain the failure');
  }

  async close() {
    if (!this.closed) this.process.stdin.end();
    const timer = setTimeout(() => { if (!this.closed) this.process.kill(); }, 3000);
    const result = await this.exit;
    clearTimeout(timer);
    this.lines.close();
    allClients.delete(this);
    return result;
  }
}

function assertCompact(value, expectedStatus) {
  assert.deepEqual(Object.keys(value).sort(), ['id', 'status', 'updated_at']);
  assert.ok(typeof value.id === 'string' || typeof value.id === 'number');
  assert.equal(value.status, expectedStatus);
  assert.ok(Number.isFinite(Date.parse(value.updated_at)), 'Server timestamp must be parseable');
}

async function check(name, action) {
  const started = Date.now();
  try {
    await action();
    results.push({ name, status: 'PASS', duration_ms: Date.now() - started });
    console.log(`PASS ${name}`);
  } catch (error) {
    results.push({ name, status: 'FAIL', duration_ms: Date.now() - started, error: error.stack ?? String(error) });
    throw error;
  }
}

let client;
let id;
const task = {
  project_path: projectPath,
  task_key: 'feature:中文-sync',
  title: '跨会话保存中文长标题与项目进展，更新同一任务且不产生重复记录',
  status: 'todo',
  progress: '已明确要求记录到看板。',
  branch: 'feature/中文-kanban',
};

try {
  await check('initialize, initialized notification, ping, and exactly three tool schemas', async () => {
    client = new McpClient();
    const initialized = await client.initialize();
    assert.ok(typeof initialized.instructions === 'string' && initialized.instructions.length < 2500);
    const ping = await client.rpc('ping');
    assert.deepEqual(ping.result, {});
    const response = await client.rpc('tools/list');
    assert.ok(!response.error);
    const tools = response.result.tools;
    assert.deepEqual(tools.map((tool) => tool.name).sort(), ['task_archive', 'task_list', 'task_upsert']);
    for (const tool of tools) {
      assert.equal(tool.inputSchema.type, 'object');
      assert.ok(tool.inputSchema.properties);
    }
    const upsertSchema = tools.find((tool) => tool.name === 'task_upsert').inputSchema;
    assert.deepEqual([...upsertSchema.required].sort(), ['progress', 'project_path', 'status', 'task_key', 'title']);
    assert.deepEqual([...upsertSchema.properties.status.enum].sort(), ['blocked', 'done', 'in_progress', 'todo']);
    const listSchema = tools.find((tool) => tool.name === 'task_list').inputSchema;
    assert.equal(listSchema.properties.limit.default, 20);
    assert.equal(listSchema.properties.limit.maximum, 100);
  });

  await check('create and idempotent update preserve identity and Unicode data', async () => {
    const created = await client.call('task_upsert', task);
    assertCompact(created, 'todo');
    id = created.id;
    const updated = await client.call('task_upsert', { ...task, status: 'in_progress', progress: '已开始实现，正在分别验证“协议”和界面。' });
    assertCompact(updated, 'in_progress');
    assert.equal(updated.id, id);
    const repeated = await client.call('task_upsert', { ...task, status: 'in_progress', progress: '已开始实现，正在分别验证“协议”和界面。' });
    assert.equal(repeated.id, id);
    const list = await client.call('task_list', { project_path: projectPath });
    assert.equal(list.items.length, 1);
    assert.equal(list.items[0].id, id);
    assert.equal(list.items[0].task_key, task.task_key);
    assert.equal(list.items[0].title, task.title);
    assert.equal(list.items[0].branch, task.branch);
    assert.equal(list.items[0].progress, '已开始实现，正在分别验证“协议”和界面。');
    assert.equal(list.next_offset, null);
  });

  await check('blocked, complete, reopen, and default unfinished filtering', async () => {
    assertCompact(await client.call('task_upsert', { ...task, status: 'blocked', progress: '等待所需输入。' }), 'blocked');
    assert.equal((await client.call('task_list', { project_path: projectPath, status: 'blocked' })).items[0].id, id);
    const done = await client.call('task_upsert', { ...task, status: 'done', progress: '实现和自动检查已完成。' });
    assertCompact(done, 'done');
    assert.equal(done.id, id);
    assert.equal((await client.call('task_list', { project_path: projectPath })).items.length, 0);
    const completed = await client.call('task_list', { project_path: projectPath, include_done: true });
    assert.equal(completed.items.length, 1);
    assert.equal(completed.items[0].status, 'done');
    assert.equal((await client.call('task_list', { project_path: projectPath, status: 'done' })).items[0].id, id);
    const reopened = await client.call('task_upsert', { ...task, status: 'in_progress', progress: '发现新的复验需求，重新打开原任务。' });
    assertCompact(reopened, 'in_progress');
    assert.equal(reopened.id, id);
  });

  await check('archive and restore retain the same task', async () => {
    const archived = await client.call('task_archive', { project_path: projectPath, task_key: task.task_key });
    assertCompact(archived, 'in_progress');
    assert.equal(archived.id, id);
    assert.equal((await client.call('task_list', { project_path: projectPath })).items.length, 0);
    const list = await client.call('task_list', { project_path: projectPath, include_archived: true });
    assert.equal(list.items.length, 1);
    assert.equal(list.items[0].archived, true);
    await client.expectToolError('task_upsert', { ...task, progress: '归档项应先恢复再更新。' });
    const restored = await client.call('task_archive', { project_path: projectPath, task_key: task.task_key, archived: false });
    assertCompact(restored, 'in_progress');
    assert.equal(restored.id, id);
    assert.equal((await client.call('task_list', { project_path: projectPath })).items[0].archived, false);
  });

  await check('restart restores committed tasks without a running GUI', async () => {
    await client.close();
    client = new McpClient();
    await client.initialize();
    const persisted = await client.call('task_list', { project_path: projectPath });
    assert.equal(persisted.items.length, 1);
    assert.equal(persisted.items[0].id, id);
    assert.equal(persisted.items[0].status, 'in_progress');
  });

  await check('four MCP processes concurrently write 32 distinct tasks without loss', async () => {
    const workers = Array.from({ length: 4 }, () => new McpClient());
    try {
      await Promise.all(workers.map((worker) => worker.initialize()));
      await Promise.all(workers.map(async (worker, workerIndex) => {
        for (let index = 0; index < 8; index += 1) {
          const key = `worker-${workerIndex}-task-${index}`;
          const result = await worker.call('task_upsert', {
            project_path: concurrentProject, task_key: key, title: `并发任务 ${key}`,
            status: 'todo', progress: '独立进程已写入。',
          });
          assertCompact(result, 'todo');
        }
      }));
    } finally {
      await Promise.all(workers.map((worker) => worker.close()));
    }
    const all = await client.call('task_list', { project_path: concurrentProject, limit: 100 });
    assert.equal(all.items.length, 32);
    assert.equal(new Set(all.items.map((item) => item.id)).size, 32);
    assert.equal(new Set(all.items.map((item) => item.task_key)).size, 32);
  });

  await check('default page size 20, explicit pagination, and project filters', async () => {
    const first = await client.call('task_list', { project_path: concurrentProject });
    assert.equal(first.items.length, 20);
    assert.equal(first.next_offset, 20);
    const second = await client.call('task_list', { project_path: concurrentProject, offset: first.next_offset });
    assert.equal(second.items.length, 12);
    assert.equal(second.next_offset, null);
    assert.equal(new Set([...first.items, ...second.items].map((item) => item.id)).size, 32);
    const bounded = await client.call('task_list', { project_path: concurrentProject, limit: 7 });
    assert.equal(bounded.items.length, 7);
    assert.equal(bounded.next_offset, 7);
    assert.equal((await client.call('task_list', { project_path: projectPath })).items.length, 1);
  });

  await check('invalid inputs and unknown methods/tools return explicit errors', async () => {
    await client.expectToolError('task_upsert', { ...task, status: 'finished' });
    await client.expectToolError('task_upsert', { ...task, title: '' });
    await client.expectToolError('task_upsert', { ...task, task_key: '' });
    await client.expectToolError('task_upsert', { ...task, progress: '第一行\n第二行' });
    await client.expectToolError('task_upsert', { project_path: projectPath, task_key: 'missing-fields' });
    await client.expectToolError('task_list', { limit: 101 });
    await client.expectToolError('task_list', { offset: -1 });
    await client.expectToolError('task_archive', { project_path: projectPath, task_key: 'does-not-exist' });
    await client.expectToolError('not_a_tool', {});
    const response = await client.rpc('not_a_method');
    assert.equal(response.error?.code, -32601);
    assert.equal((await client.call('task_list', { project_path: projectPath })).items.length, 1);
  });

  await check('compatible 2024-11-05 protocol handshake', async () => {
    const older = new McpClient();
    try {
      await older.initialize('2024-11-05');
      assert.equal((await older.call('task_list', { project_path: projectPath })).items.length, 1);
    } finally {
      await older.close();
    }
  });

  await check('database write failure is returned to the initialized client', async () => {
    const databasePath = path.join(dataDir, 'agentkanban.sqlite3');
    const savedDatabase = path.join(dataDir, 'agentkanban.sqlite3.saved');
    assert.equal(path.dirname(databasePath), dataDir);
    assert.equal(path.dirname(savedDatabase), dataDir);
    await rename(databasePath, savedDatabase);
    try {
      await mkdir(databasePath);
      try {
        await client.expectToolError('task_upsert', { ...task, progress: '此写入必须因数据库不可用而失败。' });
      } finally {
        await rmdir(databasePath);
      }
    } finally {
      await rename(savedDatabase, databasePath);
    }
    const restored = await client.call('task_list', { project_path: projectPath });
    assert.equal(restored.items[0].id, id);
    assert.equal(restored.items[0].progress, '发现新的复验需求，重新打开原任务。');
  });

  await check('unavailable data directory produces an explicit failure', async () => {
    const blockedDirectory = path.join(fixtureRoot, 'not-a-directory');
    await writeFile(blockedDirectory, 'A regular file cannot be the database directory.\n');
    const broken = new McpClient(blockedDirectory);
    let failed = false;
    try {
      try {
        await broken.initialize();
        await broken.expectToolError('task_upsert', task);
        failed = true;
      } catch (error) {
        const exit = await broken.close();
        assert.ok(exit.code !== 0 || exit.error, `Expected a failed process: ${JSON.stringify(exit)}`);
        assert.ok(broken.stderr.trim().length > 0 || exit.error, 'Startup failure needs a diagnostic');
        failed = true;
      }
      assert.ok(failed);
    } finally {
      await broken.close();
    }
  });

  await check('stdout remains protocol-only and notifications have no response', async () => {
    assert.deepEqual(client.outputErrors, []);
    assert.deepEqual(client.unmatched, []);
  });
  report.status = 'PASS';
} catch (error) {
  report.status = 'FAIL';
  report.error = error.stack ?? String(error);
  console.error(report.error);
  process.exitCode = 1;
} finally {
  await Promise.all([...allClients].map((current) => current.close()));
  report.finished_at = new Date().toISOString();
  report.fixture_retained = keep || report.status !== 'PASS';
  if (!report.fixture_retained) {
    assert.equal(path.dirname(path.resolve(fixtureRoot)), path.resolve(tmpdir()));
    assert.ok(path.basename(fixtureRoot).startsWith('agentkanban-mcp-check-'));
    await rm(fixtureRoot, { recursive: true, force: true });
  }
  const json = `${JSON.stringify(report, null, 2)}\n`;
  if (reportPath) {
    await mkdir(path.dirname(reportPath), { recursive: true });
    await writeFile(reportPath, json, 'utf8');
    console.log(`Report: ${reportPath}`);
  }
  if (report.fixture_retained) {
    await writeFile(path.join(fixtureRoot, 'verification.json'), json, 'utf8');
    console.log(`Retained fixture: ${fixtureRoot}`);
  }
  console.log(`${report.status}: ${results.filter((item) => item.status === 'PASS').length}/${results.length} checks`);
}
