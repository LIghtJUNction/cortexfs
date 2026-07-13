import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import useBaseUrl from '@docusaurus/useBaseUrl';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import CortexCurtain, {
  type CortexCurtainCopy,
  type CortexCurtainMode,
} from '../components/CortexCurtain';
import type {ReactElement} from 'react';

type CoreObject = {
  name: CortexCurtainMode;
  title: string;
  text: string;
  code: string;
};

type QuickStep = {
  title: string;
  text: string;
  code: string;
};

type Copy = {
  description: string;
  eyebrow: string;
  title: string;
  lead: string;
  watchDemo: string;
  install: string;
  github: string;
  proofLabel: string;
  proofs: string[];
  rootLabel: string;
  rootAria: string;
  curtain: CortexCurtainCopy;
  demoLabel: string;
  demoTitle: string;
  demoCaption: string;
  demoAria: string;
  demoFallback: string;
  evidenceLabel: string;
  evidenceCommand: string;
  evidencePath: string;
  coreLabel: string;
  coreTitle: string;
  coreLead: string;
  objects: CoreObject[];
  authorityLabel: string;
  authorityTitle: string;
  authorityText: string;
  see: string;
  resolve: string;
  execute: string;
  intersection: string;
  neutralLabel: string;
  neutralTitle: string;
  neutralText: string;
  neutralFootnote: string;
  quickLabel: string;
  quickTitle: string;
  quickLead: string;
  steps: QuickStep[];
  quickLink: string;
  closingLabel: string;
  closingTitle: string;
  closingText: string;
  readSpec: string;
};

const roots = ['status', 'bin', 'model', 'agent', 'tool', 'home', 'shared'];

const zh: Copy = {
  description:
    'CortexFS 在 /ctx 挂载模型、agent、tool 与持久 session，形成可查看、执行、安全控制和审计的小型 Unix ABI。',
  eyebrow: 'Filesystem as Agent OS',
  title: 'Agent runtime 不该藏在数据库里。',
  lead:
    'CortexFS 在 /ctx 挂载模型、agent、tool 和持久 session——一个可用 ls、cat、执行、安全控制和审计的小型 Unix ABI。',
  watchDemo: '观看真实 Demo',
  install: '开始安装',
  github: 'GitHub',
  proofLabel: 'CortexFS 运行时证据',
  proofs: ['7 个稳定根名称', '通过 FUSE 挂载', '受策略约束', '持久 JSONL'],
  rootLabel: '冻结的 v1 根 ABI',
  rootAria: 'CortexFS 稳定根名称',
  curtain: {
    sceneLabel: '交互式 CortexFS 挂载对象视图',
    selectorLabel: '选择 CortexFS 对象视图',
    mountLabel: 'FUSE MOUNT',
    rootLabel: '7 个稳定根名称',
    captions: {
      model: 'model 是纯推理文件：读取元数据，执行一次推理。',
      agent: 'agent 是可执行对象与 socket，也是受 policy 约束的编排者。',
      tool: 'tool 是能力端点，由 tsh 沿 CTX_PATH 发现并执行。',
      session: 'session 是普通文件：原始 JSONL 持久，prompt context 可重建。',
    },
  },
  demoLabel: '真实运行，而非概念图',
  demoTitle: '看 runtime 真正在工作。',
  demoCaption:
    '真实 CortexFS CLI 与 coder agent 交互；命令、tool 调用和 session 路径仍然直接可见。',
  demoAria: 'CortexFS coder agent 真实运行演示',
  demoFallback: '打开 CortexFS 演示视频',
  evidenceLabel: '命令',
  evidenceCommand: '启动并进入人类聊天界面',
  evidencePath: '持久消息路径',
  coreLabel: '三类可执行对象，一份持久历史',
  coreTitle: 'Runtime 的关键部分都能被直接操作。',
  coreLead:
    'CortexFS 只提供三类可执行对象，并用普通文件保存 session 历史，让 shell、权限和普通文件承担它们已经擅长的工作。',
  objects: [
    {
      name: 'model',
      title: '模型是纯推理文件',
      text:
        '读取文件得到元数据，执行文件完成一次推理。供应商连接与 API 格式差异留在统一模型 ABI 之后。',
      code: '/ctx/model/main',
    },
    {
      name: 'agent',
      title: 'Agent 是受策略约束的编排者',
      text:
        'Agent 以可执行对象和 socket 暴露；Linux 身份、mount、cwd、model 与 policy 共同限定它能看见和执行什么。',
      code: '/ctx/agent/coder  +  coder.sock',
    },
    {
      name: 'tool',
      title: 'Tool 是可执行能力端点',
      text: 'Agent 通过 tsh 调用 tool。tsh 只沿 CTX_PATH 查找能力，不会回退到 host PATH。',
      code: 'CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool',
    },
    {
      name: 'session',
      title: 'Session 保存原始历史',
      text: 'messages.jsonl 与 events.jsonl 持久追加；prompt context 是可丢弃、可重建的工作集。',
      code: 'session/default/{messages,events}.jsonl',
    },
  ],
  authorityLabel: '可审计的权限边界',
  authorityTitle: 'Linux 权限与 runtime policy 在路径上相交。',
  authorityText:
    '可见性不是 prompt 里的承诺。mount、uid/gid/mode bits、CortexFS policy、CTX_PATH 与 noexec 共同形成 agent 的实际权限。',
  see: '看见',
  resolve: '发现',
  execute: '执行',
  intersection: '最终权限 = 所有边界的交集',
  neutralLabel: '供应商中立',
  neutralTitle: '变化的 API 之上，保持一个稳定 ABI。',
  neutralText:
    'CortexFS 负责路径、对象生命周期、权限与 session 语义；Rig 负责供应商连接和 API 事件适配。供应商细节不会膨胀成新的根目录。',
  neutralFootnote: 'root 只包含稳定对象类别',
  quickLabel: '快速开始',
  quickTitle: '三步，从安装包到真实对话。',
  quickLead: '下面就是当前 README 使用的安装、挂载与交互命令。',
  steps: [
    {title: '安装', text: '从 AUR 安装 CortexFS。', code: 'paru -S cortexfs-git'},
    {
      title: '挂载并验证',
      text: '启动 systemd FUSE 挂载，然后检查有效状态。',
      code: 'sudo systemctl enable --now cortexfs.service\nctx doctor',
    },
    {
      title: '初始化并聊天',
      text: '生成默认对象，启动 coder，再进入首选人类聊天界面。',
      code: 'ctx bootstrap\nctx agent start coder\nctx agent chat coder',
    },
  ],
  quickLink: '阅读完整安装文档',
  closingLabel: '让 runtime 回到 Unix',
  closingTitle: '小到足以审计，实用到足以构建。',
  closingText: '从 v1 spec 理解稳定边界，在 GitHub 查看实现，或直接挂载 /ctx 开始使用。',
  readSpec: '阅读 v1 Spec',
};

const en: Copy = {
  description:
    'CortexFS mounts models, agents, tools, and durable sessions at /ctx as a small Unix ABI you can inspect, execute, secure, and audit.',
  eyebrow: 'Filesystem as Agent OS',
  title: 'Your agent runtime shouldn’t hide inside a database.',
  lead:
    'CortexFS mounts models, agents, tools, and durable sessions at /ctx — a small Unix ABI you can ls, cat, execute, secure, and audit.',
  watchDemo: 'Watch the real demo',
  install: 'Install CortexFS',
  github: 'GitHub',
  proofLabel: 'CortexFS runtime proof',
  proofs: ['7 stable root names', 'FUSE mounted', 'policy-bound', 'durable JSONL'],
  rootLabel: 'Frozen v1 root ABI',
  rootAria: 'CortexFS stable root names',
  curtain: {
    sceneLabel: 'Interactive CortexFS mount object view',
    selectorLabel: 'Select a CortexFS object view',
    mountLabel: 'FUSE MOUNT',
    rootLabel: '7 STABLE ROOT NAMES',
    captions: {
      model: 'A model is a pure inference file: read metadata, execute one inference.',
      agent: 'An agent is an executable object and socket: a policy-bound orchestrator.',
      tool: 'A tool is a capability endpoint resolved and executed by tsh through CTX_PATH.',
      session: 'A session is ordinary files: raw JSONL stays durable; prompt context rebuilds.',
    },
  },
  demoLabel: 'Running product, not a diagram',
  demoTitle: 'Watch the runtime while it works.',
  demoCaption:
    'The real CortexFS CLI talking to a coder agent; commands, tool calls, and session paths remain directly visible.',
  demoAria: 'Live CortexFS coder agent demonstration',
  demoFallback: 'Open the CortexFS demo video',
  evidenceLabel: 'Command',
  evidenceCommand: 'Start and enter the human chat UI',
  evidencePath: 'Durable message path',
  coreLabel: 'Three executable objects, one durable history',
  coreTitle: 'The important parts of the runtime stay directly operable.',
  coreLead:
    'CortexFS exposes three executable object classes and keeps session history in ordinary files, letting shells and permissions do the jobs they already understand.',
  objects: [
    {
      name: 'model',
      title: 'Models are pure inference files',
      text:
        'Read the file for metadata; execute it for one-shot inference. Provider connections and API formats stay behind the unified model ABI.',
      code: '/ctx/model/main',
    },
    {
      name: 'agent',
      title: 'Agents orchestrate under policy',
      text:
        'An agent is exposed as an executable object and socket. Its Linux identity, mounts, cwd, model, and policy bound what it can see and run.',
      code: '/ctx/agent/coder  +  coder.sock',
    },
    {
      name: 'tool',
      title: 'Tools are executable capability endpoints',
      text:
        'Agents invoke tools through tsh. tsh resolves capabilities only through CTX_PATH and never falls back to the host PATH.',
      code: 'CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool',
    },
    {
      name: 'session',
      title: 'Sessions preserve raw history',
      text:
        'messages.jsonl and events.jsonl remain durable and append-only; prompt context is a disposable, rebuildable working set.',
      code: 'session/default/{messages,events}.jsonl',
    },
  ],
  authorityLabel: 'Inspectable authority',
  authorityTitle: 'Linux permissions and runtime policy meet at the path.',
  authorityText:
    'Visibility is not a promise in a prompt. Mounts, uid/gid/mode bits, CortexFS policy, CTX_PATH, and noexec combine into the authority an agent actually receives.',
  see: 'See',
  resolve: 'Resolve',
  execute: 'Execute',
  intersection: 'Effective authority = the intersection of every boundary',
  neutralLabel: 'Provider-neutral by design',
  neutralTitle: 'One stable ABI above changing APIs.',
  neutralText:
    'CortexFS owns paths, object lifecycle, permissions, and session semantics. Rig owns provider connections and API event adaptation. Provider details never expand into new root directories.',
  neutralFootnote: 'root contains stable object classes only',
  quickLabel: 'Quick start',
  quickTitle: 'From package to a live chat in three steps.',
  quickLead: 'These are the install, mount, and interaction commands from the current README.',
  steps: [
    {title: 'Install', text: 'Install CortexFS from the AUR.', code: 'paru -S cortexfs-git'},
    {
      title: 'Mount and verify',
      text: 'Start the systemd FUSE mount, then inspect effective health.',
      code: 'sudo systemctl enable --now cortexfs.service\nctx doctor',
    },
    {
      title: 'Bootstrap and chat',
      text: 'Materialize the defaults, start coder, and enter the preferred human chat UI.',
      code: 'ctx bootstrap\nctx agent start coder\nctx agent chat coder',
    },
  ],
  quickLink: 'Read the full installation guide',
  closingLabel: 'Bring the runtime back to Unix',
  closingTitle: 'Small enough to audit. Useful enough to build on.',
  closingText:
    'Read the v1 spec for the stable boundary, inspect the implementation on GitHub, or mount /ctx and start using it.',
  readSpec: 'Read the v1 spec',
};

function BrandLockup(): ReactElement {
  const logoSrc = useBaseUrl('/img/cortexfs-logo.jpg');
  return (
    <div className="cortexBrandLockup" aria-label="CortexFS">
      <img src={logoSrc} alt="" />
      <span>Cor<i>TeX</i>fs</span>
    </div>
  );
}

function ProductVideo({copy}: {copy: Copy}): ReactElement {
  const mp4Src = useBaseUrl('/video/cortexfs-demo.mp4');
  const webmSrc = useBaseUrl('/video/cortexfs-demo.webm');
  const posterSrc = useBaseUrl('/video/cortexfs-demo-poster.jpg');

  return (
    <figure className="cortexFilmFrame">
      <video
        aria-describedby="cortex-film-caption"
        aria-label={copy.demoAria}
        controls
        muted
        playsInline
        preload="metadata"
        poster={posterSrc}
      >
        <source src={webmSrc} type="video/webm" />
        <source src={mp4Src} type="video/mp4" />
        <a href={mp4Src}>{copy.demoFallback}</a>
      </video>
      <figcaption id="cortex-film-caption">
        <span aria-hidden="true" />
        {copy.demoCaption}
      </figcaption>
    </figure>
  );
}

function Hero({copy}: {copy: Copy}): ReactElement {
  return (
    <section className="cortexHero" aria-labelledby="cortex-hero-title">
      <div className="container cortexHeroGrid">
        <div className="cortexHeroCopy">
          <BrandLockup />
          <p className="cortexEyebrow">{copy.eyebrow}</p>
          <h1 id="cortex-hero-title">{copy.title}</h1>
          <p className="cortexLead">{copy.lead}</p>
          <div className="cortexActions">
            <a className="cortexButton cortexButtonPrimary" href="#demo">{copy.watchDemo}</a>
            <Link className="cortexButton" to="/docs/getting-started">{copy.install}</Link>
            <a
              className="cortexTextLink cortexExternal"
              href="https://github.com/LIghtJUNction/cortexfs"
              rel="noreferrer"
              target="_blank"
            >
              {copy.github}<span aria-hidden="true"> ↗</span>
            </a>
          </div>
          <div className="cortexProofLedger" aria-label={copy.proofLabel}>
            {copy.proofs.map((proof, index) => (
              <span key={proof}><b>{String(index + 1).padStart(2, '0')}</b>{proof}</span>
            ))}
          </div>
          <div className="cortexRootLine" aria-label={copy.rootAria}>
            <span>{copy.rootLabel}</span>
            <code>$ ls /ctx</code>
            <div>{roots.map((root) => <code key={root}>{root}</code>)}</div>
          </div>
        </div>
        <CortexCurtain copy={copy.curtain} />
      </div>
    </section>
  );
}

function DemoFilm({copy}: {copy: Copy}): ReactElement {
  return (
    <section className="cortexFilm" id="demo" aria-labelledby="cortex-demo-title">
      <div className="container">
        <header className="cortexFilmHeader">
          <p className="cortexSectionLabel">{copy.demoLabel}</p>
          <h2 id="cortex-demo-title">{copy.demoTitle}</h2>
        </header>
        <div className="cortexFilmGrid">
          <ProductVideo copy={copy} />
          <aside className="cortexFilmEvidence" aria-label={copy.proofLabel}>
            <div>
              <span>01 / {copy.evidenceLabel}</span>
              <strong>{copy.evidenceCommand}</strong>
              <code>$ ctx agent start coder{`\n`}$ ctx agent chat coder</code>
            </div>
            <div>
              <span>02 / {copy.evidencePath}</span>
              <strong>/ctx/agent/coder</strong>
              <code>/ctx/home/&lt;uid&gt;/agent/coder/session/default/messages.jsonl</code>
            </div>
            <div>
              <span>03 / tsh</span>
              <strong>CTX_PATH</strong>
              <code>tsh → fs.read</code>
            </div>
          </aside>
        </div>
      </div>
    </section>
  );
}

function Specimens({copy}: {copy: Copy}): ReactElement {
  return (
    <section className="cortexSpecimens" aria-labelledby="cortex-core-title">
      <div className="container">
        <header className="cortexEditorialHeader">
          <div>
            <p className="cortexSectionLabel">{copy.coreLabel}</p>
            <h2 id="cortex-core-title">{copy.coreTitle}</h2>
          </div>
          <p>{copy.coreLead}</p>
        </header>
        <div className="cortexSpecimenList">
          {copy.objects.map((object, index) => (
            <article key={object.name}>
              <span>{String(index + 1).padStart(2, '0')}</span>
              <code>{object.name}</code>
              <div><h3>{object.title}</h3><p>{object.text}</p></div>
              <pre><code>{object.code}</code></pre>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}

function Boundaries({copy}: {copy: Copy}): ReactElement {
  return (
    <section className="cortexBoundaries" aria-labelledby="cortex-authority-title">
      <div className="container">
        <div className="cortexEquationGrid">
          <article>
            <p className="cortexSectionLabel">{copy.authorityLabel}</p>
            <h2 id="cortex-authority-title">{copy.authorityTitle}</h2>
            <strong>mount ∩ uid/gid/mode ∩ policy ∩ CTX_PATH ∩ noexec</strong>
            <p>{copy.authorityText}</p>
          </article>
          <article>
            <p className="cortexSectionLabel">{copy.neutralLabel}</p>
            <h2>{copy.neutralTitle}</h2>
            <strong>one ABI / changing APIs</strong>
            <p>{copy.neutralText}</p>
          </article>
        </div>
        <div className="cortexInstrumentStrip">
          <div><span>{copy.see}</span><code>mount ∩ uid/gid/mode</code></div>
          <div><span>{copy.resolve}</span><code>CTX_PATH</code></div>
          <div><span>{copy.execute}</span><code>policy ∩ noexec</code></div>
          <div><span>{copy.intersection}</span><code>{copy.neutralFootnote}</code></div>
        </div>
      </div>
    </section>
  );
}

function QuickStart({copy}: {copy: Copy}): ReactElement {
  return (
    <section className="cortexQuick" aria-labelledby="cortex-quick-title">
      <div className="container">
        <header className="cortexEditorialHeader cortexQuickHeader">
          <div>
            <p className="cortexSectionLabel">{copy.quickLabel}</p>
            <h2 id="cortex-quick-title">{copy.quickTitle}</h2>
          </div>
          <div>
            <p>{copy.quickLead}</p>
            <Link className="cortexInlineLink" to="/docs/getting-started">{copy.quickLink} →</Link>
          </div>
        </header>
        <ol className="cortexSteps">
          {copy.steps.map((step, index) => (
            <li key={step.title}>
              <span>{String(index + 1).padStart(2, '0')}</span>
              <h3>{step.title}</h3>
              <p>{step.text}</p>
              <pre><code>{step.code}</code></pre>
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}

function Closing({copy}: {copy: Copy}): ReactElement {
  return (
    <section className="cortexClosing" aria-labelledby="cortex-closing-title">
      <div className="container cortexClosingGrid">
        <div>
          <p className="cortexSectionLabel">{copy.closingLabel}</p>
          <h2 id="cortex-closing-title">{copy.closingTitle}</h2>
          <p>{copy.closingText}</p>
        </div>
        <div className="cortexClosingActions">
          <Link className="cortexButton cortexButtonLight" to="/docs/spec/root-abi">{copy.readSpec}</Link>
          <a
            className="cortexButton cortexButtonCoal cortexExternal"
            href="https://github.com/LIghtJUNction/cortexfs"
            rel="noreferrer"
            target="_blank"
          >
            {copy.github}<span aria-hidden="true"> ↗</span>
          </a>
          <Link className="cortexButton cortexButtonCoal" to="/docs/getting-started">{copy.install}</Link>
        </div>
      </div>
    </section>
  );
}

export default function Home(): ReactElement {
  const {i18n} = useDocusaurusContext();
  const copy = i18n.currentLocale === 'en' ? en : zh;

  return (
    <Layout title={copy.eyebrow} description={copy.description}>
      <main className="cortexHome">
        <Hero copy={copy} />
        <DemoFilm copy={copy} />
        <Specimens copy={copy} />
        <Boundaries copy={copy} />
        <QuickStart copy={copy} />
        <Closing copy={copy} />
      </main>
    </Layout>
  );
}
