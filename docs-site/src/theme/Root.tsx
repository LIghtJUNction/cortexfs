import type {ReactElement, ReactNode} from 'react';
import {useEffect} from 'react';

type RootProps = {
  children: ReactNode;
};

const INITIALS_SCROLL_Y = 72;
const MARK_SCROLL_Y = 220;

function brandPhaseForScroll(scrollY: number): string {
  if (scrollY >= MARK_SCROLL_Y) {
    return 'mark';
  }
  if (scrollY >= INITIALS_SCROLL_Y) {
    return 'initials';
  }
  return 'full';
}

export default function Root({children}: RootProps): ReactElement {
  useEffect(() => {
    const root = document.documentElement;
    let frame = 0;

    const updatePhase = () => {
      frame = 0;
      root.dataset.cortexBrandPhase = brandPhaseForScroll(window.scrollY);
    };

    const scheduleUpdate = () => {
      if (frame === 0) {
        frame = window.requestAnimationFrame(updatePhase);
      }
    };

    updatePhase();
    window.addEventListener('scroll', scheduleUpdate, {passive: true});

    return () => {
      window.removeEventListener('scroll', scheduleUpdate);
      if (frame !== 0) {
        window.cancelAnimationFrame(frame);
      }
      delete root.dataset.cortexBrandPhase;
    };
  }, []);

  return <>{children}</>;
}
