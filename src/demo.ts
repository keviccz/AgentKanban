// Browser-only sample board for screenshots and docs: open the preview with ?demo
// (optional &theme=dark&lang=en&accent=purple&activity=wave). The desktop app never uses it.
import { defaults, type Preferences, type Snapshot, type Task } from './types';

const params = new URLSearchParams(location.search);
export const demo = params.has('demo');
const english = params.get('lang') === 'en';
const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();
const text = (zh: string, en: string) => english ? en : zh;

let nextId = 1;
function task(projectId: number, fields: Partial<Task> & Pick<Task, 'title' | 'status'>): Task {
  const id = nextId++;
  const updated = fields.agent_updated_at ?? ago(90);
  return {
    id, project_id: projectId, task_key: `demo:${id}`, progress: '', branch: null, updated_at: updated, archived: false,
    request: '', agent: null, next_action: '', needs_input: '', deliverables: [], review_status: 'none', user_note: '',
    agent_updated_at: updated, steps: [], review_withdrawn_at: null, goal: '', acceptance: [], ...fields,
  };
}
const steps = (done: number, total: number) => Array.from({ length: total }, (_, index) => ({ title: `step ${index + 1}`, status: index < done ? 'done' as const : index === done ? 'in_progress' as const : 'todo' as const }));

export const demoSnapshot: Snapshot = {
  revision: 1,
  projects: [
    { id: 1, name: 'web-shop', path: 'D:\\code\\web-shop', archived_count: 3, tasks: [
      task(1, { title: text('结账页支持优惠码', 'Coupon codes at checkout'), status: 'in_progress', agent: 'Claude Code', agent_updated_at: ago(2), branch: 'feat/coupons', steps: steps(3, 5),
        progress: text('优惠码校验与金额计算已完成，正在补结账页交互和测试。', 'Validation and totals are done; wiring the checkout UI and tests now.') }),
      task(1, { title: text('移动端导航溢出', 'Mobile navigation overflow'), status: 'in_progress', agent: 'Codex', agent_updated_at: ago(48), steps: steps(1, 3),
        progress: text('已定位到菜单宽度计算，等待确认是否保留旧版菜单。', 'Found the width bug; waiting on whether to keep the old menu.'),
        needs_input: text('是否保留旧版汉堡菜单？', 'Keep the old hamburger menu?') }),
      task(1, { title: text('升级到 React 19', 'Upgrade to React 19'), status: 'done', agent: 'Claude Code', agent_updated_at: ago(180), review_status: 'pending', steps: steps(4, 4),
        progress: text('依赖与类型已升级，全部测试通过。', 'Dependencies and types upgraded; all tests pass.') }),
    ] },
    { id: 2, name: 'data-pipeline', path: 'D:\\code\\data-pipeline', archived_count: 0, tasks: [
      task(2, { title: text('夜间同步改为增量', 'Incremental nightly sync'), status: 'in_progress', agent: 'Codex', agent_updated_at: ago(6), steps: steps(1, 4),
        progress: text('已加变更游标，正在迁移历史数据。', 'Change cursor added; migrating historical rows.') }),
      task(2, { title: text('补齐 ETL 单元测试', 'Unit tests for the ETL jobs'), status: 'todo', agent_updated_at: null, updated_at: ago(30),
        progress: text('等待 Agent 接手', 'Waiting for an Agent') }),
    ] },
    { id: 3, name: 'docs-site', path: 'D:\\code\\docs-site', archived_count: 5, tasks: [
      task(3, { title: text('首页改版', 'Landing page refresh'), status: 'done', agent: 'Cursor', agent_updated_at: ago(600), review_status: 'accepted' }),
      task(3, { title: text('搜索支持中文分词', 'CJK-aware search'), status: 'done', agent: 'Claude Code', agent_updated_at: ago(1500), review_status: 'pending' }),
    ] },
  ],
};

export function demoPreferences(): Preferences {
  const theme = params.get('theme');
  const accent = params.get('accent');
  const activity = params.get('activity');
  return {
    ...defaults,
    theme: theme === 'dark' || theme === 'light' ? theme : 'light',
    language: english ? 'en' : 'zh',
    accent: (accent ?? 'teal') as Preferences['accent'],
    activity_style: (activity ?? 'pulse') as Preferences['activity_style'],
    project_colors: { 1: 'blue', 2: 'green' },
    pinned_projects: [1],
  };
}
