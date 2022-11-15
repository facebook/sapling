/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */


import React from 'react';
import clsx from 'clsx';
import Layout from '@theme/Layout';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import useBaseUrl from '@docusaurus/useBaseUrl';
import styles from './styles.module.css';

type FeatureItem = Readonly<{
  title: string;
  svg: React.ComponentType<React.ComponentProps<'svg'>>;
  description: JSX.Element;
}>;

const features: ReadonlyArray<FeatureItem> = [
  {
    title: 'Intuitive UI at Scale',
    svg: null,
    description: (
      <>
        Sapling makes development easier than ever before by simplifying common
        source control workflows and providing easy-to-use UIs while also
        scaling to the largest repositories in the world.
      </>
    ),
  },
  {
    title: 'Integrate with Git',
    svg: null,
    description: (
      <>
        Sapling client also supports cloning and interacting with Git
        repositories and can be used by individual developers to work with
        GitHub and other Git hosting services.
      </>
    ),
  },
  {
    title: 'Stack Your Work',
    svg: null,
    description: (
      <>
        Sapling provides convenient methods for stacking, iterating on, and
        submitting your work for code review. It removes the hassle of branches
        and the dreaded "detached HEAD" state.
      </>
    ),
  },
];

function Feature({svg: Svg, title, description}: FeatureItem) {
  return (
    <div className={clsx('col col--4', styles.feature)}>
      {Svg != null && (
        <div className="text--center">
          <Svg className={styles.featureImage} role="img" />
        </div>
      )}
      <h3>{title}</h3>
      <p>{description}</p>
    </div>
  );
}

export default function Home(): JSX.Element {
  const {siteConfig} = useDocusaurusContext();

  return (
    <Layout
      title={`${siteConfig.title} from Meta`}
      description="A Scalable, User-Friendly SCM">
      <header className={clsx('hero hero--primary', styles.heroBanner)}>
        <div className="container">
          <img src="img/Sapling_logo-white.svg" width="300" height="87" alt="Sapling Logo"/>
          <p className="hero__subtitle">{siteConfig.tagline}</p>
          <div className={styles.buttons}>
            <Link
              className={clsx(
                'button button--outline button--secondary button--lg',
                styles.getStarted,
              )}
              to={useBaseUrl('docs/introduction/getting-started')}>
              Get Started
            </Link>
          </div>
        </div>
      </header>
      <main>
        {features && features.length > 0 && (
          <section className={styles.features}>
            <div className="container">
              <div className="row">
                {features.map(({title, svg, description}) => (
                  <Feature
                    key={title}
                    title={title}
                    svg={svg}
                    description={description}
                  />
                ))}
              </div>
            </div>
          </section>
        )}
      </main>
    </Layout>
  );
}
