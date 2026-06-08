import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';

import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">{siteConfig.tagline}</p>
        <div className={styles.buttons}>
          <Link
            className="button button--secondary button--lg"
            to="/docs/intro">
            Read the docs
          </Link>
          <Link
            className={clsx(
              'button button--outline button--secondary button--lg',
              styles.secondaryButton,
            )}
            to="/docs/bun-template">
            Bun template
          </Link>
        </div>
      </div>
    </header>
  );
}

export default function Home(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title="CortexFS documentation"
      description="Provider-neutral AI API filesystem documentation for CortexFS.">
      <HomepageHeader />
      <main>
        <section className="container margin-top--lg margin-bottom--xl">
          <div className="row">
            <article className="col col--4">
              <h2>File ABI</h2>
              <p>
                Submit native API JSON through stable FUSE paths and inspect
                routes, responses, audit events, and exports with ordinary Unix tools.
              </p>
            </article>
            <article className="col col--4">
              <h2>Provider-neutral</h2>
              <p>
                API format, provider instance, model, route, secret status, and
                health are separate filesystem objects.
              </p>
            </article>
            <article className="col col--4">
              <h2>Bun ready</h2>
              <p>
                Use the bundled Bun client template to discover CortexFS routes
                and submit requests through the file ABI or local API.
              </p>
            </article>
          </div>
        </section>
      </main>
    </Layout>
  );
}
