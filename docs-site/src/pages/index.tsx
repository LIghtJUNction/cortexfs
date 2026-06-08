import type {ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import Translate, {translate} from '@docusaurus/Translate';
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
            <Translate id="homepage.hero.readDocs">阅读文档</Translate>
          </Link>
          <Link
            className={clsx(
              'button button--outline button--secondary button--lg',
              styles.secondaryButton,
            )}
            to="/docs/bun-template">
            <Translate id="homepage.hero.bunTemplate">Bun 模板</Translate>
          </Link>
        </div>
      </div>
    </header>
  );
}

const sections = [
  {
    href: '/docs/getting-started/quick-start',
    title: translate({
      id: 'homepage.section.gettingStarted.title',
      message: '快速开始',
    }),
    body: translate({
      id: 'homepage.section.gettingStarted.body',
      message: '构建、挂载、设置 CTX_HOME，并提交第一条文件 ABI 请求。',
    }),
  },
  {
    href: '/docs/concepts/filesystem-abi',
    title: translate({
      id: 'homepage.section.concepts.title',
      message: '核心概念',
    }),
    body: translate({
      id: 'homepage.section.concepts.body',
      message: '理解 filesystem ABI、format/provider/model 分层、space 和安全边界。',
    }),
  },
  {
    href: '/docs/api/file-api',
    title: translate({
      id: 'homepage.section.api.title',
      message: 'API 表面',
    }),
    body: translate({
      id: 'homepage.section.api.body',
      message: '文件 API、本地 HTTP/Unix API、thread 和 batch 的提交语义。',
    }),
  },
  {
    href: '/docs/providers/provider-instances',
    title: translate({
      id: 'homepage.section.providers.title',
      message: 'Provider 与路由',
    }),
    body: translate({
      id: 'homepage.section.providers.body',
      message: '多个 base_url/key 的 provider instance、fallback、priority、weight 和 secrets。',
    }),
  },
  {
    href: '/docs/bun-template',
    title: translate({
      id: 'homepage.section.integrations.title',
      message: '集成',
    }),
    body: translate({
      id: 'homepage.section.integrations.body',
      message: 'Bun 客户端模板、外部编排器、agent、tool 和 MCP 接入方式。',
    }),
  },
  {
    href: '/docs/operations/audit-export',
    title: translate({
      id: 'homepage.section.operations.title',
      message: '运维',
    }),
    body: translate({
      id: 'homepage.section.operations.body',
      message: '审计、导出、live tests 和开发约束。',
    }),
  },
];

export default function Home(): ReactNode {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title={translate({
        id: 'homepage.layout.title',
        message: 'CortexFS 文档',
      })}
      description={translate({
        id: 'homepage.layout.description',
        message: 'CortexFS provider-neutral AI API filesystem 文档。',
      })}>
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
