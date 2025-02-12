/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {ColumnAlignmentProps} from './Flex';

import * as stylex from '@stylexjs/stylex';
import {Column, Row} from './Flex';
import {spacing} from './theme/tokens.stylex';

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
  spaceBetween: {
    justifyContent: 'space-between',
  },
});

export type PanelInfo = {render: () => ReactNode; label: ReactNode};
export function Panels<T extends string>({
  panels,
  xstyle,
  tabXstyle,
  tabListXstyle,
  alignmentProps,
  active,
  onSelect,
  tabListOptionalComponent,
}: {
  panels: Record<T, PanelInfo>;
  xstyle?: stylex.StyleXStyles;
  tabXstyle?: stylex.StyleXStyles;
  tabListXstyle?: stylex.StyleXStyles;
  alignmentProps?: ColumnAlignmentProps;
  active: T;
  onSelect: (item: T) => void;
  tabListOptionalComponent?: ReactNode;
}) {
  return (
    <Column xstyle={xstyle} {...(alignmentProps ?? {alignStart: true})}>
      <Row xstyle={[styles.tabList, styles.spaceBetween, tabListXstyle]} role="tablist">
        <Row>
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
        {tabListOptionalComponent}
      </Row>
      <div role="tabpanel" {...stylex.props(styles.tabpanel)}>
        {panels[active]?.render()}
      </div>
    </Column>
  );
}
