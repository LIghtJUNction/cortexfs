import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import {useCallback, useEffect, useState, type KeyboardEvent, type ReactElement} from 'react';
import '../css/mount.css';

type RootId = 'status' | 'bin' | 'model' | 'agent' | 'tool' | 'home' | 'shared';

type RootCopy = {
  id: RootId;
  role: string;
  text: string;
  listing: string[];
  command: string;
};

type Copy = {
  description: string;
  eyebrow: string;
  title: string;
  lead: string;
  listingLabel: string;
  inspectLabel: string;
  childrenLabel: string;
  commandLabel: string;
  prompt: string;
  install: string;
  spec: string;
  roots: RootCopy[];
};

const zh: Copy = {
  description: '在浏览器里列出 /ctx：点选稳定根名称，查看路径、子项与可执行命令。',
  eyebrow: 'FUSE 挂载演示',
  title: '运行时，已挂载。',
  lead: 'CortexFS 把 model、agent、tool 和持久 session 放在 /ctx。下面是一份可点选的目录：路径仍是路径，命令仍是命令。',
  listingLabel: '稳定根 ABI',
  inspectLabel: '对象检查器',
  childrenLabel: '目录',
  commandLabel: '命令',
  prompt: 'ls /ctx',
  install: '开始安装',
  spec: '阅读根 ABI',
  roots: [
    {
      id: 'status',
      role: '当前挂载状态',
      text: 'status 是挂载健康面：读它，确认 FUSE 会话是否仍在、策略是否已加载。',
      listing: ['ready', 'mount', 'policy', 'version'],
      command: 'cat /ctx/status',
    },
    {
      id: 'bin',
      role: 'ABI 辅助命令',
      text: 'bin 放 CortexFS 自己的命令，而不是 host PATH。ctx、tsh、ctxterm 从这里被发现。',
      listing: ['ctx', 'tsh', 'ctxterm', 'ctxchat'],
      command: 'ls /ctx/bin',
    },
    {
      id: 'model',
      role: '纯推理文件',
      text: '读取得到元数据，执行完成一次推理。供应商连接留在统一模型 ABI 之后。',
      listing: ['main', 'main.d/metadata', 'main.d/limit', 'main.d/cap'],
      command: '/ctx/model/main',
    },
    {
      id: 'agent',
      role: '受策略约束的编排者',
      text: 'Agent 以可执行对象和 socket 暴露。Linux 身份、mount、cwd、model 与 policy 共同限定权限。',
      listing: ['coder', 'coder.sock', 'reviewer', 'worker'],
      command: 'ctx agent chat coder',
    },
    {
      id: 'tool',
      role: '可执行能力端点',
      text: 'tsh 只沿 CTX_PATH 发现 tool，不会回退到 host PATH。能力是文件，不是隐藏 RPC。',
      listing: ['fs.read', 'tsh', 'bash', 'agent.start'],
      command: 'CTX_PATH=/ctx/tool tsh fs.read',
    },
    {
      id: 'home',
      role: '按 Linux 用户隔离',
      text: 'home/<uid> 保存用户级 model、agent、tool 与 session。原始 JSONL 在这里追加，prompt context 可重建。',
      listing: ['<uid>/model', '<uid>/agent/coder', '<uid>/tool', '<uid>/agent/coder/session/default'],
      command: 'ls /ctx/home/$UID/agent',
    },
    {
      id: 'shared',
      role: '共享工作区',
      text: 'shared 是用户与 agent 的共同空间：文档镜像、项目目录，以及需要越过 home 边界的文件。',
      listing: ['cortexfs-docs/README.md', 'cortexfs-docs/man/', 'project-a/'],
      command: 'ls /ctx/shared',
    },
  ],
};

const en: Copy = {
  description: 'List /ctx in the browser: select a stable root, inspect its path, children, and a live command.',
  eyebrow: 'FUSE mount demo',
  title: 'Runtime, mounted.',
  lead: 'CortexFS keeps models, agents, tools, and durable sessions under /ctx. This page is a clickable listing — paths stay paths, commands stay commands.',
  listingLabel: 'Stable root ABI',
  inspectLabel: 'Object inspector',
  childrenLabel: 'Listing',
  commandLabel: 'Command',
  prompt: 'ls /ctx',
  install: 'Install CortexFS',
  spec: 'Read the root ABI',
  roots: [
    {
      id: 'status',
      role: 'live mount health',
      text: 'status is the mount health surface: read it to confirm the FUSE session is up and policy is loaded.',
      listing: ['ready', 'mount', 'policy', 'version'],
      command: 'cat /ctx/status',
    },
    {
      id: 'bin',
      role: 'ABI helper commands',
      text: 'bin holds CortexFS commands, not the host PATH. ctx, tsh, and ctxterm are discovered from here.',
      listing: ['ctx', 'tsh', 'ctxterm', 'ctxchat'],
      command: 'ls /ctx/bin',
    },
    {
      id: 'model',
      role: 'pure inference file',
      text: 'Read metadata; execute once for inference. Provider connections stay behind the unified model ABI.',
      listing: ['main', 'main.d/metadata', 'main.d/limit', 'main.d/cap'],
      command: '/ctx/model/main',
    },
    {
      id: 'agent',
      role: 'policy-bound orchestrator',
      text: 'An agent is an executable object and a socket. Linux identity, mounts, cwd, model, and policy bound its authority.',
      listing: ['coder', 'coder.sock', 'reviewer', 'worker'],
      command: 'ctx agent chat coder',
    },
    {
      id: 'tool',
      role: 'executable capability',
      text: 'tsh resolves tools only through CTX_PATH and never falls back to the host PATH. Capabilities are files, not hidden RPC.',
      listing: ['fs.read', 'tsh', 'bash', 'agent.start'],
      command: 'CTX_PATH=/ctx/tool tsh fs.read',
    },
    {
      id: 'home',
      role: 'per-Linux-user isolation',
      text: 'home/<uid> holds user models, agents, tools, and sessions. Raw JSONL appends here; prompt context rebuilds.',
      listing: ['<uid>/model', '<uid>/agent/coder', '<uid>/tool', '<uid>/agent/coder/session/default'],
      command: 'ls /ctx/home/$UID/agent',
    },
    {
      id: 'shared',
      role: 'shared workspace',
      text: 'shared is the common floor for users and agents: documentation mirrors, project trees, and files that must cross home.',
      listing: ['cortexfs-docs/README.md', 'cortexfs-docs/man/', 'project-a/'],
      command: 'ls /ctx/shared',
    },
  ],
};

function Listing({
  copy,
  selected,
  onSelect,
}: {
  copy: Copy;
  selected: RootId;
  onSelect: (id: RootId) => void;
}): ReactElement {
  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      const ids = copy.roots.map((root) => root.id);
      const index = ids.indexOf(selected);
      if (event.key === 'ArrowDown' || event.key === 'j') {
        event.preventDefault();
        onSelect(ids[(index + 1) % ids.length]);
      } else if (event.key === 'ArrowUp' || event.key === 'k') {
        event.preventDefault();
        onSelect(ids[(index - 1 + ids.length) % ids.length]);
      } else if (event.key === 'Home') {
        event.preventDefault();
        onSelect(ids[0]);
      } else if (event.key === 'End') {
        event.preventDefault();
        onSelect(ids[ids.length - 1]);
      }
    },
    [copy.roots, onSelect, selected],
  );

  return (
    <div
      aria-label={copy.listingLabel}
      className="cortexMountPageList"
      onKeyDown={onKeyDown}
      role="group"
      tabIndex={0}
    >
      <header>
        <span>{copy.listingLabel}</span>
        <code>$ {copy.prompt}</code>
      </header>
      {copy.roots.map((root, index) => {
        const active = root.id === selected;
        return (
          <button
            aria-pressed={active}
            className="cortexMountPageRow"
            key={root.id}
            onClick={() => onSelect(root.id)}
            type="button"
          >
            <b>{String(index + 1).padStart(2, '0')}</b>
            <code>{root.id}</code>
            <span>{root.role}</span>
          </button>
        );
      })}
    </div>
  );
}

function Inspector({copy, root}: {copy: Copy; root: RootCopy}): ReactElement {
  return (
    <aside aria-label={copy.inspectLabel} className="cortexMountPageInspect">
      <header>
        <span>FUSE MOUNT</span>
        <strong>/ctx/{root.id}</strong>
        <code>{root.role}</code>
      </header>
      <p aria-live="polite" key={root.id}>{root.text}</p>
      <div>
        <span>{copy.childrenLabel}</span>
        <ul>
          {root.listing.map((entry) => (
            <li key={entry}>
              <code>/ctx/{root.id}/{entry}</code>
            </li>
          ))}
        </ul>
      </div>
      <div>
        <span>{copy.commandLabel}</span>
        <pre>
          <code>
            <i>$</i> {root.command}
          </code>
        </pre>
      </div>
    </aside>
  );
}

export default function MountPage(): ReactElement {
  const {i18n} = useDocusaurusContext();
  const copy = i18n.currentLocale === 'en' ? en : zh;
  const [selected, setSelected] = useState<RootId>('model');
  const root = copy.roots.find((entry) => entry.id === selected) ?? copy.roots[2];

  useEffect(() => {
    document.documentElement.dataset.cortexMountPage = 'true';
    return () => {
      delete document.documentElement.dataset.cortexMountPage;
    };
  }, []);

  return (
    <Layout title={copy.eyebrow} description={copy.description}>
      <main className="cortexMountPage">
        <section className="cortexMountPageHero">
          <div className="container">
            <p className="cortexEyebrow">{copy.eyebrow}</p>
            <h1>{copy.title}</h1>
            <p className="cortexLead">{copy.lead}</p>
            <div className="cortexActions">
              <Link className="cortexButton cortexButtonPrimary" to="/docs/getting-started">
                {copy.install}
              </Link>
              <Link className="cortexButton" to="/docs/spec/root-abi">
                {copy.spec}
              </Link>
            </div>
          </div>
        </section>
        <section className="cortexMountPageStage" aria-label={copy.inspectLabel}>
          <div className="container cortexMountPageGrid">
            <Listing copy={copy} onSelect={setSelected} selected={selected} />
            <Inspector copy={copy} root={root} />
          </div>
        </section>
      </main>
    </Layout>
  );
}
