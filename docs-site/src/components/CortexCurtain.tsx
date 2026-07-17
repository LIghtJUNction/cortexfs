import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactElement,
} from 'react';

export type CortexCurtainMode = 'model' | 'agent' | 'tool' | 'session';

export type CortexCurtainCopy = {
  sceneLabel: string;
  selectorLabel: string;
  mountLabel: string;
  rootLabel: string;
  captions: Record<CortexCurtainMode, string>;
};

type CurtainStyle = CSSProperties & {
  '--curtain-i': number;
  '--curtain-length': string;
};

const modes: CortexCurtainMode[] = ['model', 'agent', 'tool', 'session'];

const glyphs: Record<CortexCurtainMode, string[]> = {
  model: [
    '/ctx/model/main',
    'metadata · limit · cap',
    'exec → JSONL',
    'pure inference',
    'stdin · stdout',
    'gpt-5.6-sol',
    'read → metadata',
    'run → inference',
    'route stays behind ABI',
    'one-shot executable',
    'status · log',
    'kimi-k3',
    '/ctx/model',
  ],
  agent: [
    '/ctx/agent/coder',
    'coder.sock',
    'policy-bound',
    'uid · gid · mode',
    'mount · cwd',
    'model · policy',
    'ctx agent start coder',
    'ctx agent chat coder',
    'orchestrator',
    'socket JSONL',
    'visible tools',
    'owned session',
    '/ctx/agent',
  ],
  tool: [
    '/ctx/tool/fs.read',
    'tsh → fs.read',
    'CTX_PATH',
    'executable endpoint',
    'schema · cap',
    'policy · status',
    'never host PATH',
    'tool/fs.read',
    'stdin · stdout',
    'capability',
    'discover · execute',
    'tsh',
    '/ctx/tool',
  ],
  session: [
    'messages.jsonl',
    'events.jsonl',
    'append-only',
    'context/ rebuildable',
    'latest.md',
    'state · cwd',
    '/ctx/home/<uid>',
    'agent/coder/session',
    'raw history durable',
    'prompt disposable',
    'default/messages.jsonl',
    'ordinary files',
    'session/default',
  ],
};

const columnLengths = [292, 348, 318, 386, 332, 414, 360, 398, 326, 376, 306, 350, 282];

export default function CortexCurtain({copy}: {copy: CortexCurtainCopy}): ReactElement {
  const [active, setActive] = useState<CortexCurtainMode>('model');
  const visualRef = useRef<HTMLDivElement>(null);
  const columnRefs = useRef<Array<HTMLSpanElement | null>>([]);
  const frameRef = useRef<number | null>(null);
  const pendingPointer = useRef({x: 0, y: 0});
  const pointerDisabled = useRef(true);

  const resetColumns = useCallback(() => {
    columnRefs.current.forEach((column) => {
      column?.style.setProperty('--bend-x', '0px');
      column?.style.setProperty('--bend-y', '0px');
      column?.style.setProperty('--bend-rotate', '0deg');
      column?.style.setProperty('--bend-opacity', '1');
    });
  }, []);

  const applyPointer = useCallback(() => {
    frameRef.current = null;
    const visual = visualRef.current;
    if (!visual || pointerDisabled.current) {
      resetColumns();
      return;
    }

    const rect = visual.getBoundingClientRect();
    const localX = pendingPointer.current.x - rect.left;
    const localY = pendingPointer.current.y - rect.top;
    const curtainLeft = rect.width * 0.13;
    const curtainWidth = rect.width * 0.74;
    const radius = Math.max(72, curtainWidth * 0.2);

    columnRefs.current.forEach((column, index) => {
      if (!column) {
        return;
      }

      const columnX = curtainLeft + ((index + 0.5) / glyphs[active].length) * curtainWidth;
      const distance = Math.abs(localX - columnX);
      const influence = Math.max(0, 1 - distance / radius);
      const direction = localX >= columnX ? -1 : 1;
      const vertical = Math.max(-0.5, Math.min(0.7, (localY - rect.height * 0.42) / rect.height));

      column.style.setProperty('--bend-x', `${direction * influence * 24}px`);
      column.style.setProperty('--bend-y', `${vertical * influence * 22}px`);
      column.style.setProperty('--bend-rotate', `${direction * influence * 4.2}deg`);
      column.style.setProperty('--bend-opacity', `${1 - influence * 0.16}`);
    });
  }, [active, resetColumns]);

  const handlePointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (pointerDisabled.current) {
      return;
    }

    pendingPointer.current = {x: event.clientX, y: event.clientY};
    if (frameRef.current === null) {
      frameRef.current = window.requestAnimationFrame(applyPointer);
    }
  };

  const handlePointerLeave = () => {
    if (frameRef.current !== null) {
      window.cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    }
    resetColumns();
  };

  useEffect(() => {
    const media = window.matchMedia('(prefers-reduced-motion: reduce), (pointer: coarse)');
    const updatePointerMode = () => {
      pointerDisabled.current = media.matches;
      if (media.matches) {
        resetColumns();
      }
    };

    updatePointerMode();
    media.addEventListener('change', updatePointerMode);
    return () => media.removeEventListener('change', updatePointerMode);
  }, [resetColumns]);

  useEffect(() => {
    resetColumns();
  }, [active, resetColumns]);

  useEffect(
    () => () => {
      if (frameRef.current !== null) {
        window.cancelAnimationFrame(frameRef.current);
      }
    },
    [],
  );

  return (
    <section className="cortexCurtain" aria-label={copy.sceneLabel}>
      <div
        className="cortexCurtainVisual"
        ref={visualRef}
        onPointerLeave={handlePointerLeave}
        onPointerMove={handlePointerMove}
        aria-hidden="true"
      >
        <div className="cortexCurtainShadow" />
        <div className="cortexMountPlate">
          <span className="cortexMountBolt cortexMountBoltLeft" />
          <strong>/ctx</strong>
          <span>{copy.mountLabel}</span>
          <code>{active}</code>
          <span className="cortexMountBolt cortexMountBoltRight" />
        </div>
        <div className="cortexGlyphCurtain">
          {glyphs[active].map((glyph, index) => {
            const style: CurtainStyle = {
              '--curtain-i': index,
              '--curtain-length': `${columnLengths[index]}px`,
            };
            return (
              <span
                className="cortexCurtainColumn"
                key={`${active}-${glyph}`}
                ref={(node) => {
                  columnRefs.current[index] = node;
                }}
                style={style}
              >
                {glyph}
              </span>
            );
          })}
        </div>
        <div className="cortexCurtainRoots">
          <span>{copy.rootLabel}</span>
          <code>status · bin · model · agent · tool · home · shared</code>
        </div>
      </div>
      <div className="cortexCurtainControls" role="group" aria-label={copy.selectorLabel}>
        {modes.map((mode) => (
          <button
            aria-pressed={active === mode}
            key={mode}
            onClick={() => setActive(mode)}
            type="button"
          >
            {mode}
          </button>
        ))}
      </div>
      <p className="cortexCurtainCaption" aria-live="polite">
        <span>{active}</span>
        {copy.captions[active]}
      </p>
    </section>
  );
}
