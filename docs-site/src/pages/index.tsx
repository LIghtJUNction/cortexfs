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

const sections = [
  {
    title: 'Getting Started',
    href: '/docs/getting-started/quick-start',
    body: '构建、挂载、设置 CTX_HOME，并提交第一条文件 ABI 请求。',
  },
  {
    title: 'Concepts',
    href: '/docs/concepts/filesystem-abi',
    body: '理解 filesystem ABI、format/provider/model 分层、space 和安全边界。',
  },
  {
    title: 'API Surface',
    href: '/docs/api/file-api',
    body: '文件 API、本地 HTTP/Unix API、thread 和 batch 的提交语义。',
  },
  {
    title: 'Providers and Routing',
    href: '/docs/providers/provider-instances',
    body: '多个 base_url/key 的 provider instance、fallback、priority、weight 和 secrets。',
  },
  {
    title: 'Integrations',
    href: '/docs/bun-template',
    body: 'Bun 客户端模板、外部编排器、agent、tool 和 MCP 接入方式。',
  },
  {
    title: 'Operations',
    href: '/docs/operations/audit-export',
    body: '审计、导出、live tests 和开发约束。',
  },
];

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
            {sections.map((section) => (
              <article className="col col--4 margin-bottom--lg" key={section.title}>
                <div className={styles.sectionCard}>
                  <h2>
                    <Link to={section.href}>{section.title}</Link>
                  </h2>
                  <p>{section.body}</p>
                </div>
              </article>
            ))}
          </div>
        </section>
      </main>
    </Layout>
  );
}
