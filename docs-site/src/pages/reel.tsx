import {useEffect, type CSSProperties, type ReactElement} from 'react';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import '../css/reel.css';

type ReelColumnStyle = CSSProperties & {
  '--reel-column': number;
  '--reel-height': string;
  '--reel-peel-x': string;
  '--reel-peel-y': string;
  '--reel-peel-r': string;
};

const roots = ['status', 'bin', 'model', 'agent', 'tool', 'home', 'shared'];
const curtain = roots.flatMap((root) => [root, `/ctx/${root}`]);

type ReelCopy = {
  filmLabel: string;
  filmTagline: string;
  filmCode: string;
  stageAriaLabel: string;
  phase1Label: string;
  phase1Title: string;
  phase1Roots: string;
  phase2Label: string;
  phase2Title: string;
  phase2Role: string;
  phase3Label: string;
  commandCode: string;
  plateLabel: string;
  rootsLabel: string;
  phase4Label: string;
  phase4Expression: string;
  phase4Title: string;
  phase5Title: string;
  phase5Attribution: string;
  specimens: Array<[string, string]>;
};

const en: ReelCopy = {
  filmLabel: '20 SECOND SYSTEM FILM',
  filmTagline: 'CortexFS twenty-second product reel',
  filmCode: 'FUSE · UNIX ABI · JSONL',
  stageAriaLabel: 'FUSE filesystem interface for agent runtimes',
  phase1Label: 'FUSE FILESYSTEM INTERFACE',
  phase1Title: 'Runtime, mounted.',
  phase1Roots: 'status · bin · model · agent · tool · home · shared',
  plateLabel: 'FUSE MOUNT',
  rootsLabel: '7 ROOTS',
  phase2Label: 'THE EXECUTABLE SURFACE',
  phase2Title: 'model · agent · tool',
  phase2Role: 'three executable object classes',
  phase3Label: 'WATCH THE RUNTIME WORK',
  commandCode: 'ctx agent chat coder',
  phase4Label: 'AUTHORITY IS AN INTERSECTION',
  phase4Expression: 'mount ∩ uid/gid/mode ∩ policy',
  phase4Title: 'Provider-neutral. One stable ABI.',
  phase5Title: 'Your agent runtime shouldn’t hide inside a database.',
  phase5Attribution: 'lightjunction.github.io/cortexfs',
  specimens: [
    ['model', 'pure inference'],
    ['agent', 'policy-bound orchestrator'],
    ['tool', 'executable capability'],
    ['session', 'durable history'],
  ],
};

const zh: ReelCopy = {
  filmLabel: '20 秒系统短片',
  filmTagline: 'CortexFS 20 秒系统短片',
  filmCode: 'FUSE · UNIX ABI · JSONL',
  stageAriaLabel: '面向 agent 运行时的 FUSE 文件系统接口',
  phase1Label: 'FUSE 文件系统界面',
  phase1Title: '运行时，已挂载。',
  phase1Roots: 'status · bin · model · agent · tool · home · shared',
  plateLabel: 'FUSE 挂载',
  rootsLabel: '7 个根',
  phase2Label: '可执行对象界面',
  phase2Title: 'model · agent · tool',
  phase2Role: '三类可执行对象',
  phase3Label: '观看运行时执行',
  commandCode: 'ctx agent chat coder',
  phase4Label: '权限是交集',
  phase4Expression: 'mount ∩ uid/gid/mode ∩ policy',
  phase4Title: '供应商中立。一个稳定 ABI。',
  phase5Title: '你的代理运行时不应藏在数据库里。',
  phase5Attribution: 'lightjunction.github.io/cortexfs',
  specimens: [
    ['model', '纯推理文件'],
    ['agent', '受策略约束的编排者'],
    ['tool', '可执行能力端点'],
    ['session', '持久历史'],
  ],
};

export default function Reel(): ReactElement {
  const {i18n} = useDocusaurusContext();
  const copy = i18n.currentLocale === 'en' ? en : zh;

  useEffect(() => {
    const root = document.documentElement;
    const autoplay = new URLSearchParams(window.location.search).get('autoplay') !== '0';

    root.dataset.cortexReelReady = 'true';
    if (autoplay) {
      root.dataset.cortexReel = 'play';
    } else {
      delete root.dataset.cortexReel;
    }

    return () => {
      delete root.dataset.cortexReel;
      delete root.dataset.cortexReelReady;
    };
  }, []);

  return (
    <main className="cortexReel" aria-label={copy.filmTagline}>
      <div className="cortexReelGrain" aria-hidden="true" />
      <header className="cortexReelHeader">
        <strong>Cor<i>TeX</i>fs</strong>
        <span>{copy.filmLabel}</span>
        <code>{copy.filmCode}</code>
      </header>

      <section className="cortexReelStage" aria-label={copy.stageAriaLabel}>
        <div className="cortexReelAssembly" data-reel-animate aria-hidden="true">
          <div className="cortexReelLongShadow" />
          <div className="cortexReelPlate">
            <span>{copy.plateLabel}</span>
            <strong>/ctx</strong>
            <code>{copy.rootsLabel}</code>
          </div>
          <div className="cortexReelCurtain">
            {curtain.map((text, index) => {
              const direction = index % 2 === 0 ? 1 : -1;
              const style: ReelColumnStyle = {
                '--reel-column': index,
                '--reel-height': `${28 + (index % 5) * 3.2}vh`,
                '--reel-peel-x': `${direction * (18 + (index % 4) * 8)}px`,
                '--reel-peel-y': `${-10 - (index % 5) * 7}px`,
                '--reel-peel-r': `${direction * (2 + (index % 3) * 1.2)}deg`,
              };
              return (
                <span data-reel-animate key={`${text}-${index}`} style={style}>
                  {text}
                </span>
              );
            })}
          </div>
        </div>

        <div className="cortexReelPhase cortexReelPhaseOne" data-reel-animate>
          <p>{copy.phase1Label}</p>
          <h1>{copy.phase1Title}</h1>
          <code>{copy.phase1Roots}</code>
        </div>

        <div className="cortexReelPhase cortexReelPhaseTwo" data-reel-animate>
          <p>{copy.phase2Label}</p>
          <h2>{copy.phase2Title}</h2>
          <strong>{copy.phase2Role}</strong>
        </div>

        <div className="cortexReelPhase cortexReelPhaseThree" data-reel-animate>
          <p>{copy.phase3Label}</p>
          <div className="cortexReelCommand">
            <code><span>$</span> {copy.commandCode}</code>
            <code>/ctx/home/&lt;uid&gt;/agent/coder/session/default/messages.jsonl</code>
            <code><span>tool</span> tsh → fs.read</code>
          </div>
        </div>

        <div className="cortexReelPhase cortexReelPhaseFour" data-reel-animate>
          <p>{copy.phase4Label}</p>
          <h2>{copy.phase4Expression}</h2>
          <strong>{copy.phase4Title}</strong>
        </div>

        <div className="cortexReelPhase cortexReelPhaseFive" data-reel-animate>
          <div className="cortexReelSpecimens">
            {copy.specimens.map(([name, role]) => (
              <div key={name}><code>{name}</code><span>{role}</span></div>
            ))}
          </div>
          <h2>{copy.phase5Title}</h2>
          <p>{copy.phase5Attribution}</p>
        </div>
      </section>

      <div className="cortexReelProgress" aria-hidden="true">
        <span data-reel-animate />
      </div>
    </main>
  );
}
