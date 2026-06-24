import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import HeroImage from '../../../docs/assets/cortexfs-hero.svg';
import type {ReactElement} from 'react';

export default function Home(): ReactElement {
  const {i18n} = useDocusaurusContext();
  const isEnglish = i18n.currentLocale === 'en';

  const copy = isEnglish
    ? {
        description:
          'Turn AI agents into a Linux filesystem you can install, inspect, call, and extend.',
        leadStart: 'Project models, agents, tools, and sessions as ordinary files. Start by installing CortexFS, use ',
        leadMiddle: ' to explore ',
        leadEnd: ', start terminals with ',
        leadTail: ', then attach to agent terminals or build your own tools and runtime integrations.',
        install: 'Install first',
        use: 'See how it works',
        agentSh: 'Try agent.sh',
        route: 'Learning path',
        step1Title: 'Install it',
        step1TextStart: 'Install from AUR, start the systemd service, and run ',
        step1TextEnd: ' to confirm the mount is actually working before reading the deeper design.',
        step2Title: 'Use it like Unix',
        step2TextStart: 'Use ',
        step2TextEnd: ' to find models, agents, and tools; start sandboxed agent terminals; and inspect session files after work completes.',
        step3Title: 'Then extend it',
        step3Text:
          'Extend tools, agents, provider routes, or the FUSE projection through the same file ABI and commit semantics.',
        why: 'Why it is interesting',
        whyTitle: 'Agents stop being opaque chat boxes',
        whyTextStart: 'CortexFS breaks a running agent into objects you can ',
        whyTextEnd:
          '. Models are files, agents have sockets, tools are executable capabilities, and session history is stored in ordinary directories.',
        objectModel: 'pure inference',
        objectAgent: 'sandboxed work',
        objectTool: 'executable capability',
        objectSession: 'durable history',
      }
    : {
        description: '把 AI agent 变成可以安装、查看、调用和扩展的 Linux 文件系统',
        leadStart: '把模型、agent、工具和会话投影成普通文件。你可以从安装开始，用 ',
        leadMiddle: ' 探索 ',
        leadEnd: '，用 ',
        leadTail: ' 启动 agent 终端，再逐步写自己的 tool、agent 或 runtime。',
        install: '开始安装',
        use: '看看怎么用',
        agentSh: '试试 agent.sh',
        route: '阅读路线',
        step1Title: '先装起来',
        step1TextStart: '从 AUR 安装、启动 systemd service、跑 ',
        step1TextEnd: '。先确认 /ctx 真的能工作，再看更深的设计。',
        step2Title: '像用 Unix 一样用它',
        step2TextStart: '用 ',
        step2TextEnd: ' 找模型、agent 和 tool；启动 sandboxed agent 终端；用 session 文件追踪它刚刚做过什么。',
        step3Title: '再去二次开发',
        step3Text:
          '扩展 tool、agent、provider route 或 FUSE 投影时，沿用同一套文件 ABI 和提交语义，不需要发明另一套编排入口。',
        why: '为什么有意思',
        whyTitle: 'Agent 不再只是一个黑盒聊天窗口',
        whyTextStart: 'CortexFS 把运行中的 agent 拆成可以 ',
        whyTextEnd:
          ' 的对象。模型是文件，agent 有 socket，工具是可执行能力，会话历史落在普通目录里。熟悉 Linux 的人可以直接上手；想做 runtime 的人也有稳定边界可依赖。',
        objectModel: '纯推理入口',
        objectAgent: '沙箱化任务',
        objectTool: '可执行能力',
        objectSession: '持久历史',
      };

  return (
    <Layout
      title="CortexFS"
      description={copy.description}
    >
      <main className="cortexHome">
        <section className="cortexHero">
          <div className="container cortexHeroInner">
            <div className="cortexHeroCopy">
              <p className="cortexEyebrow">Linux native Agent OS</p>
              <h1>CortexFS</h1>
              <p className="cortexLead">
                {copy.leadStart}
                <code>ctx</code>
                {copy.leadMiddle}
                <code>/ctx</code>
                {copy.leadEnd}
                <code>ctx agent</code>
                {copy.leadTail}
              </p>
              <div className="cortexActions">
                <Link className="cortexButton cortexButtonPrimary" to="/docs/getting-started">
                  {copy.install}
                </Link>
                <Link className="cortexButton" to="/docs/using-cortexfs">
                  {copy.use}
                </Link>
                <Link className="cortexButton" to="/docs/agent-sh">
                  {copy.agentSh}
                </Link>
              </div>
            </div>
            <div className="cortexHeroVisual" aria-label="CortexFS user journey">
              <HeroImage className="cortexHeroImage" aria-hidden="true" />
              <div className="cortexTerminal">
                <div className="cortexTerminalBar">
                  <span className="cortexDot" />
                  <span className="cortexDot" />
                  <span className="cortexDot" />
                </div>
                <pre>{`$ ctx doctor
ok: /ctx mounted
ok: model/main -> debug/echo
ok: agent/coder.sock ready

$ ctx agent start coder --session docs
agent=coder
session=docs
cwd=/workspace

$ ctx agent watch coder --session docs`}</pre>
              </div>
            </div>
          </div>
        </section>

        <section className="cortexBand cortexJourney">
          <div className="container">
            <p className="cortexSectionLabel">{copy.route}</p>
            <div className="cortexSteps">
              <Link className="cortexStep" to="/docs/getting-started">
                <span>01</span>
                <h2>{copy.step1Title}</h2>
                <p>
                  {copy.step1TextStart}
                  <code>ctx doctor</code>
                  {copy.step1TextEnd}
                </p>
              </Link>
              <Link className="cortexStep" to="/docs/using-cortexfs">
                <span>02</span>
                <h2>{copy.step2Title}</h2>
                <p>
                  {copy.step2TextStart}
                  <code>ctx ls</code>
                  {copy.step2TextEnd}
                </p>
              </Link>
              <Link className="cortexStep" to="/docs/developing-cortexfs">
                <span>03</span>
                <h2>{copy.step3Title}</h2>
                <p>{copy.step3Text}</p>
              </Link>
            </div>
          </div>
        </section>

        <section className="cortexBand">
          <div className="container cortexSplit">
            <div>
              <p className="cortexSectionLabel">{copy.why}</p>
              <h2>{copy.whyTitle}</h2>
              <p>
                {copy.whyTextStart}
                {isEnglish ? (
                  <>
                    <code>ls</code>, <code>cat</code>, <code>exec</code>, and{' '}
                    <code>tail</code>
                  </>
                ) : (
                  <>
                    <code>ls</code>、<code>cat</code>、<code>exec</code> 和{' '}
                    <code>tail</code>
                  </>
                )}
                {copy.whyTextEnd}
              </p>
            </div>
            <div className="cortexMiniMap" aria-label="CortexFS object map">
              <div><strong>model</strong><span>{copy.objectModel}</span></div>
              <div><strong>agent</strong><span>{copy.objectAgent}</span></div>
              <div><strong>tool</strong><span>{copy.objectTool}</span></div>
              <div><strong>session</strong><span>{copy.objectSession}</span></div>
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
