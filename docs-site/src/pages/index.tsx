import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import useBaseUrl from '@docusaurus/useBaseUrl';
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
  trusted: string;
  commandOne: string;
  commandTwo: string;
  commandThree: string;
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
  title: '直接与模型文件、agent 文件和工具文件对话',
  lead:
    'CortexFS 把模型、agent、tool、session 和运行状态投影到 /ctx。你可以进入 agent REPL，对模型文件发起推理，把工具动态加载进内存，并随时 attach 到 agent 终端观察内部发生了什么。',
  primary: '从安装开始',
  secondary: '日常使用',
  developer: '开发指南',
  trusted: '模型文件、agent 文件、tool shell、轻量沙箱',
  commandOne: '直接对话模型文件',
  commandTwo: '进入 agent REPL',
  commandThree: 'attach 到 ctxterm',
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
  developerTitle: '一条命令启动 agent',
  developerText:
    '不用接新框架，不用写 glue service。进入 agent REPL 后，模型、tool、上下文和审计仍然走同一套 CortexFS 文件 ABI。',
  developerSteps: [
    {
      title: '启动 REPL',
      text: '选择一个 agent 名称进入会话。coder 的模型、policy、可见目录和工具边界从现有 agent 配置加载。',
      code: 'ctx agent repl coder',
    },
    {
      title: '直接输入任务',
      text: '在 REPL 里像普通对话一样输入需求。文本、路径和说明进入同一条会话流，tool 调用仍受 CTX_PATH 和 policy 约束。',
      code: '> review /workspace/docs/DESIGN.md',
    },
    {
      title: '需要时观察',
      text: '运行状态仍是普通文件。需要调试时查看 prompt、history、latest output 和 audit，而不是接入另一套 dashboard。',
      code: 'ctx agent output coder',
    },
  ],
  architectureTitle: '高层抽象',
  architectureText:
    'CortexFS 是一层薄 ABI：它让模型、agent、tool、session 以同一种 Unix 形状组合，而不是把每个供应商或框架的内部状态变成新根目录。',
  manifestTitle: '不要把 runtime 藏在数据库里',
  manifestText:
    'CortexFS 的判断很简单：可见状态应该是普通文件，可执行能力应该有明确边界，agent 内部发生的事应该能被直接看见。',
  model: '纯推理入口',
  agent: '策略约束的编排者',
  tool: '可执行能力',
  session: '持久历史',
};

const en: Copy = {
  description:
    'CortexFS projects an AI agent runtime as a stable, scriptable, inspectable Linux filesystem ABI.',
  eyebrow: 'Filesystem as Agent OS',
  title: 'Talk directly to model files, agent files, and tool files',
  lead:
    'CortexFS projects models, agents, tools, sessions, and runtime state into /ctx. Enter an agent REPL, call model files, load tools into memory, and attach to the agent terminal whenever you need to see what is happening inside.',
  primary: 'Install first',
  secondary: 'Daily usage',
  developer: 'Developer guide',
  trusted: 'Model files, agent files, tool shell, lightweight sandbox',
  commandOne: 'talk to model files',
  commandTwo: 'enter the agent REPL',
  commandThree: 'attach to ctxterm',
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
  developerTitle: 'Start an agent with one command',
  developerText:
    'No new framework or glue service is required. Once you enter the agent REPL, models, tools, context, and audit still flow through the same CortexFS file ABI.',
  developerSteps: [
    {
      title: 'Start the REPL',
      text: 'Pick an agent name and enter its session. The coder model, policy, visible files, and tool boundary load from the existing agent configuration.',
      code: 'ctx agent repl coder',
    },
    {
      title: 'Type the task',
      text: 'Use the REPL like a normal conversation. Text, paths, and instructions share one stream while tool calls remain bounded by CTX_PATH and policy.',
      code: '> review /workspace/docs/DESIGN.md',
    },
    {
      title: 'Inspect when needed',
      text: 'Runtime state remains ordinary files. When debugging, inspect prompt, history, latest output, and audit instead of wiring another dashboard.',
      code: 'ctx agent output coder',
    },
  ],
  architectureTitle: 'High-level abstraction',
  architectureText:
    'CortexFS is a thin ABI that gives models, agents, tools, and sessions one Unix shape instead of turning every vendor or framework detail into a new root directory.',
  manifestTitle: 'Do not hide the runtime in a database',
  manifestText:
    'CortexFS takes a simple position: visible state should be ordinary files, executable capability should have clear boundaries, and agent internals should be directly observable.',
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

function AgentConsole({copy}: {copy: Copy}): ReactElement {
  return (
    <div className="cortexStage" aria-label="CortexFS working product mockup">
      <div className="cortexStageGlow" aria-hidden="true" />
      <div className="cortexWorkbench">
        <div className="cortexWorkbenchTop">
          <span>ctx</span>
          <strong>/ctx/agent/coder</strong>
          <em>live repl</em>
        </div>
        <div className="cortexShellLine">
          <span>$</span>
          <strong>ctx agent repl coder</strong>
        </div>
        <div className="cortexTranscript">
          <article>
            <span>user</span>
            <p>review /workspace/docs/DESIGN.md</p>
          </article>
          <article>
            <span>tool</span>
            <p>tsh fs.read docs/DESIGN.md</p>
          </article>
          <article>
            <span>usage</span>
            <p>input 912 / output 184</p>
          </article>
        </div>
        <div className="cortexCommitRail">
          {[copy.commandOne, copy.commandTwo, copy.commandThree].map((item, index) => (
            <span key={item}>
              <strong>{String(index + 1).padStart(2, '0')}</strong>
              {item}
            </span>
          ))}
        </div>
      </div>
      <div className="cortexStageCaption">
        <span>{copy.trusted}</span>
      </div>
    </div>
  );
}

function ProductDemo(): ReactElement {
  const videoSrc = useBaseUrl('/video/cortexfs-demo.webm');
  const posterSrc = useBaseUrl('/video/cortexfs-demo-poster.jpg');

  return (
    <figure className="cortexDemo">
      <video
        aria-label="CortexFS live agent demo"
        autoPlay
        controls
        loop
        muted
        playsInline
        poster={posterSrc}
      >
        <source src={videoSrc} type="video/webm" />
      </video>
      <figcaption>
        <span>ctx agent repl coder</span>
        <strong>/ctx/agent/coder</strong>
      </figcaption>
    </figure>
  );
}

function HeroBrand(): ReactElement {
  const logoSrc = useBaseUrl('/img/cortexfs-logo.svg');

  return (
    <div className="cortexHeroBrand" aria-label="CorTeXfs">
      <span className="cortexBrandStage cortexBrandFull" aria-hidden="true">
        <span className="cortexBrandAccent">C</span>
        <span>or</span>
        <span className="cortexBrandAccent">T</span>
        <span>e</span>
        <span className="cortexBrandAccent">X</span>
        <span>fs</span>
      </span>
      <span className="cortexBrandStage cortexBrandInitials" aria-hidden="true">
        <span>C</span>
        <span>T</span>
        <span>X</span>
      </span>
      <span className="cortexBrandStage cortexBrandLogo" aria-hidden="true">
        <img className="cortexLogoMark" src={logoSrc} alt="" />
        <span>ctx</span>
      </span>
    </div>
  );
}

function RootTicker(): ReactElement {
  const roots = ['status', 'bin', 'model', 'agent', 'tool', 'home', 'shared'];
  const loopedRoots = Array.from({length: 8}, () => roots).flat();

  return (
    <div className="cortexTicker" aria-label="CortexFS root ABI ticker">
      <div>
        {loopedRoots.map((root, index) => (
          <span key={`${root}-${index}`}>{root}</span>
        ))}
      </div>
    </div>
  );
}

function TrustDots(): ReactElement {
  return (
    <div className="cortexTrustDots" aria-hidden="true">
      {['/ctx', 'ABI', 'tsh', 'req'].map((label) => (
        <div key={label}>
          <span>{label}</span>
        </div>
      ))}
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
              <HeroBrand />
              <p className="cortexEyebrow">{copy.eyebrow}</p>
              <h1>{copy.title}</h1>
              <p className="cortexLead">{copy.lead}</p>
              <div className="cortexActions">
                <Link className="cortexButton cortexButtonPrimary" to="/docs/getting-started">
                  {copy.primary}
                </Link>
              </div>
              <TrustDots />
              <div className="cortexProofRow" aria-label="CortexFS root ABI">
                <span>status</span>
                <span>bin</span>
                <span>model</span>
                <span>agent</span>
                <span>tool</span>
                <span>home</span>
                <span>shared</span>
              </div>
            </div>
            <div className="cortexHeroVisual">
              <ProductDemo />
            </div>
          </div>
          <RootTicker />
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
