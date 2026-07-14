import {useEffect, type CSSProperties, type ReactElement} from 'react';
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

const specimens = [
  ['model', 'pure inference'],
  ['agent', 'policy-bound orchestrator'],
  ['tool', 'executable capability'],
  ['session', 'durable history'],
];

export default function Reel(): ReactElement {
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
    <main className="cortexReel" aria-label="CortexFS twenty-second product reel">
      <div className="cortexReelGrain" aria-hidden="true" />
      <header className="cortexReelHeader">
        <strong>Cor<i>TeX</i>fs</strong>
        <span>20 SECOND SYSTEM FILM</span>
        <code>FUSE · UNIX ABI · JSONL</code>
      </header>

      <section className="cortexReelStage" aria-label="FUSE filesystem interface for agent runtimes">
        <div className="cortexReelAssembly" data-reel-animate aria-hidden="true">
          <div className="cortexReelLongShadow" />
          <div className="cortexReelPlate">
            <span>FUSE MOUNT</span>
            <strong>/ctx</strong>
            <code>7 ROOTS</code>
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
          <p>FUSE FILESYSTEM INTERFACE</p>
          <h1>Runtime,<br />mounted.</h1>
          <code>status · bin · model · agent · tool · home · shared</code>
        </div>

        <div className="cortexReelPhase cortexReelPhaseTwo" data-reel-animate>
          <p>THE EXECUTABLE SURFACE</p>
          <h2>model · agent · tool</h2>
          <strong>three executable object classes</strong>
        </div>

        <div className="cortexReelPhase cortexReelPhaseThree" data-reel-animate>
          <p>WATCH THE RUNTIME WORK</p>
          <div className="cortexReelCommand">
            <code><span>$</span> ctx agent chat coder</code>
            <code>/ctx/home/&lt;uid&gt;/agent/coder/session/default/messages.jsonl</code>
            <code><span>tool</span> tsh → fs.read</code>
          </div>
        </div>

        <div className="cortexReelPhase cortexReelPhaseFour" data-reel-animate>
          <p>AUTHORITY IS AN INTERSECTION</p>
          <h2>mount ∩ uid/gid/mode ∩ policy</h2>
          <strong>Provider-neutral. One stable ABI.</strong>
        </div>

        <div className="cortexReelPhase cortexReelPhaseFive" data-reel-animate>
          <div className="cortexReelSpecimens">
            {specimens.map(([name, role]) => (
              <div key={name}><code>{name}</code><span>{role}</span></div>
            ))}
          </div>
          <h2>Your agent runtime shouldn’t hide inside a database.</h2>
          <p>lightjunction.github.io/cortexfs</p>
        </div>
      </section>

      <div className="cortexReelProgress" aria-hidden="true">
        <span data-reel-animate />
      </div>
    </main>
  );
}
