/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {Column, Row} from './Flex';
import {spacing} from './theme/tokens.stylex';
import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  tabList: {
    padding: '4px',
    paddingBottom: spacing.pad,
    gap: '32px',
  },
  tab: {
    color: {
      default: 'var(--panel-tab-foreground)',
      ':hover': 'var(--panel-tab-active-foreground)',
    },
    padding: '4px 0',
    backgroundColor: 'transparent',
    border: 'none',
    cursor: 'pointer',
    borderBottom: '1px solid transparent',
  },
  activeTab: {
    borderBottom: '1px solid var(--panel-tab-active-foreground)',
    color: 'var(--panel-tab-active-foreground)',
  },
  tabpanel: {
    padding: '0 6px 10px 6px',
  },
});

type PanelInfo = {render: () => ReactNode; label: ReactNode};
export function Panels<T extends string>({
  panels,
  xstyle,
  tabXstyle,
  active,
  onSelect,
}: {
  panels: Record<T, PanelInfo>;
  xstyle?: stylex.StyleXStyles;
  tabXstyle?: stylex.StyleXStyles;
  active: T;
  onSelect: (item: T) => void;
}) {
  return (
    <Column xstyle={xstyle} alignStart>
      <Row xstyle={styles.tabList} role="tablist">
        {(Object.entries(panels) as Array<[T, PanelInfo]>).map(([name, value]) => {
          return (
            <button
              role="tab"
              aria-selected={active === name}
              key={name}
              onClick={() => onSelect(name)}
              {...stylex.props(styles.tab, active === name && styles.activeTab, tabXstyle)}>
              {value.label}
            </button>
          );
        })}
      </Row>
      <div role="tabpanel" {...stylex.props(styles.tabpanel)}>
        {panels[active]?.render()}
      </div>
    </Column>
  );
}
