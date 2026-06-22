import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import type {ReactElement} from 'react';

export default function Home(): ReactElement {
  return (
    <Layout
      title="CortexFS"
      description="CortexFS v1 ABI documentation"
    >
      <main className="cortexHome">
        <section className="container cortexHero">
          <div>
            <h1>CortexFS</h1>
            <p className="cortexLead">
              A small Linux filesystem ABI for agent runtimes: model, agent,
              tool, session, policy, and shared files without provider or
              workflow internals in the root namespace.
            </p>
            <div className="cortexActions">
              <Link className="cortexButton cortexButtonPrimary" to="/docs/DESIGN">
                Read the design
              </Link>
              <Link className="cortexButton" to="/docs/spec/">
                Open the v1 spec
              </Link>
              <Link className="cortexButton" to="/docs/agent-sh">
                agent.sh
              </Link>
            </div>
          </div>
          <div className="cortexTerminal" aria-label="CortexFS ABI tree">
            <div className="cortexTerminalBar">
              <span className="cortexDot" />
              <span className="cortexDot" />
              <span className="cortexDot" />
            </div>
            <pre>{`/ctx
  status
  bin/
  model/
    qwen
    qwen.sock
    qwen.d/
  agent/
    coder
    coder.sock
    coder.d/
  tool/
  home/
  shared/`}</pre>
          </div>
        </section>
        <section className="cortexBand">
          <div className="container cortexGrid">
            <div className="cortexPanel">
              <h2>Frozen root ABI</h2>
              <p>
                The root contains stable object classes only. Provider,
                database, MCP, skill, and workflow details stay out of `/ctx`.
              </p>
            </div>
            <div className="cortexPanel">
              <h2>Unix-shaped objects</h2>
              <p>
                Executable files do work, `.sock` files stream stateful JSONL,
                and `.d/` directories hold small control files.
              </p>
            </div>
            <div className="cortexPanel">
              <h2>Thin clients</h2>
              <p>
                `ctx` and `agent.sh` are small clients over the ABI. Agent
                orchestration remains policy-bound runtime behavior.
              </p>
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
