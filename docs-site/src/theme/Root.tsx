import type {ReactElement, ReactNode} from 'react';

type RootProps = {
  children: ReactNode;
};

export default function Root({children}: RootProps): ReactElement {
  return <>{children}</>;
}
