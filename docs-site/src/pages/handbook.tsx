import {useState, type ReactElement} from 'react';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import '../css/handbook.css';

type Group = 'start' | 'operate' | 'extend' | 'understand';
type Text = [string, string];
type Guide = {path: string; group: Group; title: Text; summary: Text; keywords: string};

const groups: {id: Group; title: Text}[] = [
  {id: 'start', title: ['Get started', '开始使用']},
  {id: 'operate', title: ['Work with agents', '操作 Agent']},
  {id: 'extend', title: ['Build & evaluate', '开发与评估']},
  {id: 'understand', title: ['Understand the system', '理解系统']},
];

const guides: Guide[] = [
  {
    path: 'getting-started',
    group: 'start',
    title: ['Install & run', '安装与运行'],
    summary: ['Check Linux requirements, mount /ctx, and open your first session.', '检查 Linux 环境，挂载 /ctx，打开第一个会话。'],
    keywords: 'install setup fuse systemd doctor troubleshoot 安装 排错'
  },
  {
    path: 'packaging',
    group: 'start',
    title: ['Packages & updates', '安装包与更新'],
    summary: ['Choose a Linux package and plan a pinned, recoverable update.', '选择 Linux 安装包，规划可恢复的固定版本更新。'],
    keywords: 'deb rpm aur arch ubuntu fedora update 更新 升级'
  },
  {
    path: 'using-cortexfs',
    group: 'operate',
    title: ['Daily usage', '日常使用'],
    summary: ['Configure providers, use tools, and continue durable sessions.', '配置供应商、调用工具、继续持久会话。'],
    keywords: 'provider auth login resume secret 凭据 登录 会话 供应商'
  },
  {
    path: 'agent-sh',
    group: 'operate',
    title: ['Shell client', 'Shell 客户端'],
    summary: ['Connect a shell client to the agent socket and inspect its events.', '将 Shell 客户端连接到 Agent socket，查看交互事件。'],
    keywords: 'agent.sh bash socket 客户端'
  },
  {
    path: 'spec/agent-runtime',
    group: 'operate',
    title: ['Runtime & terminals', '运行时与终端'],
    summary: ['Understand process lifecycle, terminal access, and execution limits.', '了解进程生命周期、终端访问与执行限制。'],
    keywords: 'pty bwrap bubblewrap sandbox memory cpu stop cancel 沙箱 终端 内存 停止'
  },
  {
    path: 'spec/session-abi',
    group: 'operate',
    title: ['Sessions & history', '会话与历史'],
    summary: ['Find messages, events, indexes, and the rebuildable context.', '定位消息、事件、索引与可重建的上下文。'],
    keywords: 'jsonl messages events durable context resume 会话 历史 上下文'
  },
  {
    path: 'extensions',
    group: 'extend',
    title: ['Write an extension', '编写扩展'],
    summary: ['Add tools and agents through the existing object contracts.', '通过现有对象契约添加工具与 Agent。'],
    keywords: 'sdk plugin mcp tool 工具 扩展'
  },
  {
    path: 'channels',
    group: 'extend',
    title: ['Connect a chat channel', '接入聊天渠道'],
    summary: ['Route messaging platforms to the same agent and session interfaces.', '将消息平台接入同一套 Agent 与会话接口。'],
    keywords: 'telegram discord slack feishu lark wechat im 飞书 微信 渠道'
  },
  {
    path: 'developing-cortexfs',
    group: 'extend',
    title: ['Contributor workflow', '开发贡献流程'],
    summary: ['Build the workspace and follow repository verification conventions.', '构建工作区，遵循仓库的验证约定。'],
    keywords: 'rust cargo build test lint clippy 开发 编译 测试'
  },
  {
    path: 'evaluation',
    group: 'extend',
    title: ['Evaluate the harness', '评估 Agent Harness'],
    summary: ['Run reproducible scenarios and inspect the evidence behind each result.', '运行可复现的场景，检查每项结果的原始证据。'],
    keywords: 'harness benchmark evaluation performance test 基准 测试 评估 性能'
  },
  {
    path: 'aimock-testing',
    group: 'extend',
    title: ['Provider mock tests', '供应商模拟测试'],
    summary: ['Exercise model requests with controlled, local provider fixtures.', '使用可控的本地供应商夹具检查模型请求。'],
    keywords: 'aimock offline fixture mock 模拟 离线 测试'
  },
  {
    path: 'architecture',
    group: 'understand',
    title: ['System architecture', '系统架构'],
    summary: ['Read the responsibilities and boundaries of the runtime.', '理解运行时各部分的职责与边界。'],
    keywords: 'design linux unix architecture 架构 设计'
  },
  {
    path: 'internal-architecture',
    group: 'understand',
    title: ['Internal layers', '内部工程分层'],
    summary: ['Trace crate dependencies, error handling, and the migration plan.', '查看 crate 依赖、错误处理与迁移计划。'],
    keywords: 'kernel layers dependency crate error 内核 分层 错误'
  },
  {
    path: 'spec/root-abi',
    group: 'understand',
    title: ['Root ABI', '根目录 ABI'],
    summary: ['Start with the root entries and their filesystem semantics.', '从根目录项及其文件系统语义开始。'],
    keywords: 'root path spec filesystem 根 路径 规范'
  },
  {
    path: 'spec/object-abi',
    group: 'understand',
    title: ['Object ABI', '对象 ABI'],
    summary: ['Read the executable, socket, and control-file contract.', '阅读可执行文件、socket 与控制文件的契约。'],
    keywords: 'object model agent tool socket 对象 契约'
  },
  {
    path: 'spec/agent-tool-security',
    group: 'understand',
    title: ['Permissions & isolation', '权限与隔离'],
    summary: ['Check the agent identity, mount view, and authority boundaries.', '检查 Agent 身份、挂载视图与权限边界。'],
    keywords: 'security policy permission uid gid sandbox 安全 权限 沙箱'
  },
  {
    path: 'paths',
    group: 'understand',
    title: ['Path reference', '路径参考'],
    summary: ['Look up shared path constants and filesystem conventions.', '查找共享路径常量与文件系统约定。'],
    keywords: 'paths constants api reference 路径 常量'
  },
  {
    path: 'spec/',
    group: 'understand',
    title: ['Complete specification', '完整规范'],
    summary: ['Browse the normative ABI documents and their scope.', '浏览规范性 ABI 文档及其适用范围。'],
    keywords: 'spec reference abi protocol 规范 协议'
  },
];

export default function Handbook(): ReactElement {
  const {i18n} = useDocusaurusContext();
  const locale = i18n.currentLocale === 'zh-Hans' ? 1 : 0;
  const t = (value: Text) => value[locale];
  const [query, setQuery] = useState('');
  const [group, setGroup] = useState<Group | 'all'>('all');
  const words = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const matches = guides.filter((guide) => {
    const text = [...guide.title, ...guide.summary, guide.path, guide.keywords].join(' ').toLocaleLowerCase();
    return (group === 'all' || guide.group === group) && words.every((word) => text.includes(word));
  });
  const title = t(['The CortexFS handbook', 'CortexFS 使用手册']);

  return (
    <Layout title={title} description={t(['Find installation guides, daily workflows, architecture, and harness evaluation.', '查找安装指南、日常操作、架构设计与 Harness 评估文档。'])}>
      <main className="cortexHandbook container">
        <header className="handbookHeader">
          <div>
            <p className="cortexSectionLabel">/ctx · {t(['Documentation', '文档'])}</p>
            <h1>{title}</h1>
            <p>{t(['From your first mount to the runtime internals. Choose the task in front of you.', '从第一次挂载到运行时内部实现，从你当前要完成的任务开始。'])}</p>
          </div>
          <aside className="handbookStart" aria-label={t(['Start here', '从这里开始'])}>
            <span>01 / {t(['First visit?', '第一次使用？'])}</span>
            <Link to="/docs/getting-started">{t(['Install & run your first session', '安装并打开第一个会话'])}<span aria-hidden="true"> ↗</span></Link>
            <p>Linux · systemd · FUSE</p>
          </aside>
        </header>

        <section aria-labelledby="guide-directory">
          <div className="handbookTools">
            <div>
              <h2 id="guide-directory">{t(['Find your next step', '找到下一步'])}</h2>
              <p>{t(['Search guide titles, descriptions, and common commands.', '搜索指南标题、简介与常用命令。'])}</p>
            </div>
            <div className="handbookSearch">
              <label htmlFor="guide-search">{t(['Find a guide', '查找指南'])}</label>
              <input id="guide-search" type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t(['Try “provider”, “resume”, or “sandbox”', '试试“供应商”“resume”或“沙箱”'])} aria-describedby="guide-count" />
            </div>
          </div>
          <div className="handbookFilters" role="group" aria-label={t(['Filter guides by task', '按任务筛选指南'])}>
            {[{id: 'all' as const, title: ['All guides', '全部指南'] as Text}, ...groups].map((item) => (
              <button key={item.id} type="button" aria-pressed={group === item.id} onClick={() => setGroup(item.id)}>{t(item.title)}</button>
            ))}
          </div>
          <p className="handbookCount" id="guide-count" role="status">{t([`${matches.length} guides`, `${matches.length} 篇指南`])}</p>
          {matches.length ? (
            <div className="handbookGroups">
              {groups.map((section) => {
                const entries = matches.filter((guide) => guide.group === section.id);
                return entries.length > 0 && (
                  <section className="handbookGroup" key={section.id} aria-labelledby={`group-${section.id}`}>
                    <h3 id={`group-${section.id}`}>{t(section.title)}</h3>
                    <ul>{entries.map((guide) => (
                      <li key={guide.path}>
                        <Link className="handbookGuide" to={`/docs/${guide.path}`}>
                          <span className="handbookGuideTitle">{t(guide.title)}<span aria-hidden="true">↗</span></span>
                          <span className="handbookGuideSummary">{t(guide.summary)}</span>
                        </Link>
                      </li>
                    ))}</ul>
                  </section>
                );
              })}
            </div>
          ) : (
            <div className="handbookEmpty">
              <h3>{t(['No matching guides', '没有匹配的指南'])}</h3>
              <p>{t(['Try a shorter term or clear the filters to see every guide.', '换一个更短的词，或清除筛选查看所有指南。'])}</p>
              <button type="button" className="cortexButton" onClick={() => {setQuery(''); setGroup('all');}}>{t(['Clear filters', '清除筛选'])}</button>
            </div>
          )}
        </section>
        <footer className="handbookNote">
          <p>{t(['Something unclear or out of date?', '有内容不清楚或已经过时？'])}</p>
          <a href="https://github.com/LIghtJUNction/cortexfs/issues">{t(['Check project issues', '查看项目 Issue'])} <span aria-hidden="true">↗</span></a>
        </footer>
      </main>
    </Layout>
  );
}
