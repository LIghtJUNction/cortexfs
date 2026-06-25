import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import type {ReactElement} from 'react';

type Feature = {
  title: string;
  text: string;
  code: string;
};

type Copy = {
  description: string;
  eyebrow: string;
  title: string;
  lead: string;
  primary: string;
  secondary: string;
  developer: string;
  inspectTitle: string;
  inspectText: string;
  virtualFileTitle: string;
  virtualFileText: string;
  terminalTitle: string;
  terminalText: string;
  contextTitle: string;
  contextText: string;
  apiTitle: string;
  apiText: string;
  authorityTitle: string;
  authorityText: string;
  agentTreeTitle: string;
  agentTreeText: string;
  performanceTitle: string;
  performanceText: string;
  developerTitle: string;
  developerText: string;
  developerSteps: Feature[];
  architectureTitle: string;
  architectureText: string;
  manifestTitle: string;
  manifestText: string;
  model: string;
  agent: string;
  tool: string;
  session: string;
};

const zh: Copy = {
  description: 'CortexFS 把 AI agent runtime 投影成稳定、可脚本化、可审计的 Linux 文件系统 ABI。',
  eyebrow: 'Filesystem as Agent OS',
  title: '把 agent runtime 变成可以 ls、cat、exec 的文件系统',
  lead:
    'CortexFS 不把 AI 平台数据库搬进目录树。它只暴露少量稳定对象：model、agent、tool、session、policy 和 shared space。用户用 Unix 命令理解系统，开发者用普通文件操作扩展 agent。',
  primary: '从安装开始',
  secondary: '日常使用',
  developer: '开发指南',
  inspectTitle: '一眼可见',
  inspectText:
    '模型是文件，agent 是可执行对象和 socket，tool 是能力端点，session 是普通历史目录。隐藏状态变成可 inspect 的事实。',
  virtualFileTitle: '此文件，非彼文件',
  virtualFileText:
    '你看到的文件很可能不在硬盘上，而是 CortexFS 按需投影出的内存视图。不开、不读，就不生成；需要调试时直接 cat。',
  terminalTitle: 'tsh 跑在 ctxterm 上',
  terminalText:
    'ctxterm 拥有 PTY 生命周期，tsh 是 agent 唯一默认 shell。它只按 CTX_PATH 找 CortexFS tool，不回退到 host PATH。',
  contextTitle: '上下文窗口是工作集',
  contextText:
    'raw messages 持久保存，context pack 可重建。skill 元数据有预算，工具注入和历史消息按窗口动态裁剪。',
  apiTitle: '多 AI API 兼容',
  apiText:
    'provider 差异留在 model driver 和 route 内部。/ctx 根目录不出现 openai、ollama、mcp 这类供应商命名空间。',
  authorityTitle: '权限是交集',
  authorityText:
    'mount/chroot、Linux uid/gid/mode bits、CortexFS label policy、CTX_PATH 和 noexec 共同决定 agent 能看见和执行什么。',
  agentTreeTitle: 'agent 树是普通状态',
  agentTreeText:
    'base agent 派生 coder、reviewer 等子 agent。父子关系、生命周期、handoff 和 child result 都落在 agent/session 文件里。',
  performanceTitle: '为什么高效',
  performanceText:
    '稳定 ABI 小，发现靠目录遍历，执行走文件或 Unix socket。tool 元数据可 load/pin，未 pin 的上下文由 W-TinyLFU 回收。',
  developerTitle: '开发 agent 不需要新框架',
  developerText:
    '描述身份、启动会话、调用 tool、观察上下文，就是 CortexFS 的开发模型。agent runtime 可以简单到一个脚本，也可以复杂到完整调度器。',
  developerSteps: [
    {
      title: '定义身份',
      text: '用 agent/<name>.d/system.md、model、policy、mount、path 描述 persona、模型、可见目录和工具边界。',
      code: 'ctx set agent/coder.d/system.md "You are a careful Rust agent."',
    },
    {
      title: '发送任务',
      text: '高层入口是 agent 会话。文本、路径、图片说明都进入同一条对话流；文件本体放在可见 workspace 或 shared space。',
      code: 'ctx send coder "review /workspace/docs/DESIGN.md"',
    },
    {
      title: '观察运行',
      text: '用普通文件查看 prompt、history、latest output 和 context pack。需要旁观终端时 attach 到 ctxterm。',
      code: 'ctx agent prompt coder && ctx agent output coder',
    },
  ],
  architectureTitle: '高层抽象',
  architectureText:
    'CortexFS 是一层薄 ABI：它让模型、agent、tool、session 以同一种 Unix 形状组合，而不是把每个供应商或框架的内部状态变成新根目录。',
  manifestTitle: '不要把 runtime 藏在数据库里',
  manifestText:
    'CortexFS 的判断很简单：可见状态应该是普通文件，可执行能力应该有明确边界，每次提交都应该留下可审计事实。',
  model: '纯推理入口',
  agent: '策略约束的编排者',
  tool: '可执行能力',
  session: '持久历史',
};

const en: Copy = {
  description:
    'CortexFS projects an AI agent runtime as a stable, scriptable, inspectable Linux filesystem ABI.',
  eyebrow: 'Filesystem as Agent OS',
  title: 'An agent runtime you can ls, cat, exec, and audit',
  lead:
    'CortexFS does not mirror an AI platform database into directories. It exposes a small set of stable objects: models, agents, tools, sessions, policy, and shared space. Users understand it with Unix commands; developers extend it with ordinary file operations.',
  primary: 'Install first',
  secondary: 'Daily usage',
  developer: 'Developer guide',
  inspectTitle: 'Visible by default',
  inspectText:
    'Models are files, agents are executables and sockets, tools are capability endpoints, and sessions are ordinary history directories.',
  virtualFileTitle: 'A file, but not that kind of file',
  virtualFileText:
    'The file you see may not live on disk. CortexFS can project it from memory on demand: if nobody opens it, nothing has to be materialized.',
  terminalTitle: 'tsh runs on ctxterm',
  terminalText:
    'ctxterm owns the PTY lifecycle. tsh is the only default agent shell, resolving tools through CTX_PATH and never through host PATH.',
  contextTitle: 'Context is a working set',
  contextText:
    'Raw messages are durable. Context packs are rebuildable. Skill metadata has a budget; tool injection and history are trimmed to the window.',
  apiTitle: 'Many AI APIs, one ABI',
  apiText:
    'Provider differences stay behind model drivers and routes. The /ctx root does not grow openai, ollama, or mcp namespaces.',
  authorityTitle: 'Authority is intersection',
  authorityText:
    'mount/chroot visibility, Linux uid/gid/mode bits, CortexFS policy, CTX_PATH, and noexec all decide what an agent can see or run.',
  agentTreeTitle: 'Agent trees are files',
  agentTreeText:
    'A base agent can spawn coder or reviewer children. Parentage, lifecycle, handoff, and child results are stored in agent/session files.',
  performanceTitle: 'Why it stays fast',
  performanceText:
    'The ABI is small, discovery is directory traversal, execution is file or Unix socket I/O, and loaded tool metadata is bounded by W-TinyLFU.',
  developerTitle: 'Develop agents without a new framework',
  developerText:
    'Describe identity, start sessions, invoke tools, and inspect context. An agent runtime can be a script or a full scheduler.',
  developerSteps: [
    {
      title: 'Define identity',
      text: 'Use agent/<name>.d/system.md, model, policy, mount, and path to describe persona, model, visible files, and tool boundaries.',
      code: 'ctx set agent/coder.d/system.md "You are a careful Rust agent."',
    },
    {
      title: 'Send work',
      text: 'The high-level entry is an agent session. Text, paths, and image instructions share one conversation stream; file bytes live in visible workspace or shared space.',
      code: 'ctx send coder "review /workspace/docs/DESIGN.md"',
    },
    {
      title: 'Observe runtime',
      text: 'Use ordinary files to inspect prompt, history, latest output, and context packs. Attach to ctxterm when you need the terminal.',
      code: 'ctx agent prompt coder && ctx agent output coder',
    },
  ],
  architectureTitle: 'High-level abstraction',
  architectureText:
    'CortexFS is a thin ABI that gives models, agents, tools, and sessions one Unix shape instead of turning every vendor or framework detail into a new root directory.',
  manifestTitle: 'Do not hide the runtime in a database',
  manifestText:
    'CortexFS takes a simple position: visible state should be ordinary files, executable capability should have clear boundaries, and every submission should leave auditable facts.',
  model: 'pure inference',
  agent: 'policy-bound orchestration',
  tool: 'executable capability',
  session: 'durable history',
};

function FeatureRail({copy}: {copy: Copy}): ReactElement {
  const features = [
    [copy.inspectTitle, copy.inspectText, 'model/main'],
    [copy.virtualFileTitle, copy.virtualFileText, 'origin=virtual'],
    [copy.terminalTitle, copy.terminalText, 'ctxterm -> tsh'],
    [copy.contextTitle, copy.contextText, 'context/pack.md'],
    [copy.apiTitle, copy.apiText, 'model/<provider>/<id>'],
    [copy.authorityTitle, copy.authorityText, 'policy + mode bits'],
    [copy.agentTreeTitle, copy.agentTreeText, 'base -> coder'],
  ];

  return (
    <div className="cortexFeatureRail">
      {features.map(([title, text, tag]) => (
        <article className="cortexFeature" key={title}>
          <code>{tag}</code>
          <h3>{title}</h3>
          <p>{text}</p>
        </article>
      ))}
    </div>
  );
}

function AgentConsole(): ReactElement {
  return (
    <div className="cortexConsole" aria-label="CortexFS working console mockup">
      <div className="cortexConsoleHeader">
        <span>/ctx</span>
        <strong>agent/coder</strong>
      </div>
      <div className="cortexConsoleBody">
        <aside>
          <span className="isActive">status</span>
          <span>model</span>
          <span>agent</span>
          <span>tool</span>
          <span>session</span>
          <span>policy</span>
        </aside>
        <div className="cortexEditor">
          <div className="cortexMeta">
            <span>request</span>
            <span>atomic rename</span>
            <span>audit append</span>
          </div>
          <pre>{`$ ls /ctx
model  agent  tool  session  policy  shared

$ ctx send coder "inspect docs/DESIGN.md"
write: .tmp/9f2.req.json
rename: agent/coder/inbox/9f2.req.json

$ tail -f agent/coder/outbox/latest.json
{
  "status": "accepted",
  "native_tool": "tsh",
  "ctx_path": "tool/tsh:tool/fs.read"
}`}</pre>
        </div>
      </div>
    </div>
  );
}

export default function Home(): ReactElement {
  const {i18n} = useDocusaurusContext();
  const copy = i18n.currentLocale === 'en' ? en : zh;

  return (
    <Layout title="CortexFS" description={copy.description}>
      <main className="cortexHome">
        <section className="cortexHero">
          <div className="container cortexHeroInner">
            <div className="cortexHeroCopy">
              <p className="cortexEyebrow">{copy.eyebrow}</p>
              <h1>{copy.title}</h1>
              <p className="cortexLead">{copy.lead}</p>
              <div className="cortexActions">
                <Link className="cortexButton cortexButtonPrimary" to="/docs/getting-started">
                  {copy.primary}
                </Link>
                <Link className="cortexButton" to="/docs/using-cortexfs">
                  {copy.secondary}
                </Link>
                <Link className="cortexButton" to="/docs/developing-cortexfs">
                  {copy.developer}
                </Link>
              </div>
              <div className="cortexProofRow" aria-label="CortexFS root ABI">
                <span>model</span>
                <span>agent</span>
                <span>tool</span>
                <span>session</span>
                <span>policy</span>
              </div>
            </div>
            <div className="cortexHeroVisual">
              <AgentConsole />
            </div>
          </div>
        </section>

        <section className="cortexBand cortexArchitecture">
          <div className="container cortexSplit">
            <div>
              <p className="cortexSectionLabel">{copy.architectureTitle}</p>
              <h2>{copy.inspectTitle}</h2>
              <p>{copy.architectureText}</p>
            </div>
            <div className="cortexObjectMap" aria-label="CortexFS object model">
              <div><strong>model</strong><span>{copy.model}</span></div>
              <div><strong>agent</strong><span>{copy.agent}</span></div>
              <div><strong>tool</strong><span>{copy.tool}</span></div>
              <div><strong>session</strong><span>{copy.session}</span></div>
            </div>
          </div>
        </section>

        <section className="cortexBand">
          <div className="container">
            <FeatureRail copy={copy} />
          </div>
        </section>

        <section className="cortexBand cortexSystem">
          <div className="container cortexSystemGrid">
            <div className="cortexTerminal">
              <div className="cortexTerminalBar">
                <span />
                <span />
                <span />
              </div>
              <pre>{`$ ctx agent ps
base
└─ coder
   └─ reviewer

$ ctx agent prompt coder
native_tool=tsh
skills_budget=2%
history=bounded

$ tsh tools
tsh
fs.read
tsh.config`}</pre>
            </div>
            <div>
              <p className="cortexSectionLabel">{copy.performanceTitle}</p>
              <h2>{copy.performanceTitle}</h2>
              <p>{copy.performanceText}</p>
              <div className="cortexFlow" aria-label="CortexFS performance path">
                <span>file ABI</span>
                <span>Unix socket</span>
                <span>bounded context</span>
                <span>W-TinyLFU</span>
              </div>
            </div>
          </div>
        </section>

        <section className="cortexManifest">
          <div className="container">
            <p>{copy.manifestTitle}</p>
            <h2>{copy.manifestText}</h2>
            <Link to="/docs/DESIGN">{copy.architectureTitle}</Link>
          </div>
        </section>

        <section className="cortexBand cortexDeveloper">
          <div className="container">
            <div className="cortexDeveloperIntro">
              <p className="cortexSectionLabel">{copy.developerTitle}</p>
              <h2>{copy.developerTitle}</h2>
              <p>{copy.developerText}</p>
            </div>
            <div className="cortexSteps">
              {copy.developerSteps.map((step, index) => (
                <Link className="cortexStep" to="/docs/developing-cortexfs" key={step.title}>
                  <span>{String(index + 1).padStart(2, '0')}</span>
                  <h3>{step.title}</h3>
                  <p>{step.text}</p>
                  <pre>{step.code}</pre>
                </Link>
              ))}
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
