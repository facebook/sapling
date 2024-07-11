/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactProps} from './utils';

import {spacing} from './theme/tokens.stylex';
import * as stylex from '@stylexjs/stylex';

type ContainerProps = ReactProps<HTMLDivElement> & {xstyle?: stylex.StyleXStyles};

const styles = stylex.create({
  center: {
    display: 'flex',
    width: '100%',
    height: '100%',
    alignItems: 'center',
    justifyContent: 'center',
  },
  column: {
    flexDirection: 'column',
    alignItems: 'flex-start',
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
  },
  flex: {
    display: 'flex',
    gap: spacing.pad,
  },
  spacer: {
    flexGrow: 1,
  },
  alignStart: {
    alignItems: 'flex-start',
  },
  alignCenter: {
    alignItems: 'center',
  },
});

/** Vertical flex layout */
export function Column(
  props: ContainerProps &
    (
      | {alignStart: true; alignCenter?: undefined | false}
      | {alignStart?: undefined | false; alignCenter: true}
    ),
) {
  const {xstyle, alignStart, alignCenter, ...rest} = props;
  return (
    <div
      {...rest}
      {...stylex.props(
        styles.flex,
        styles.column,
        xstyle,
        alignStart && styles.alignStart,
        alignCenter && styles.alignCenter,
      )}
    />
  );
}

/** Horizontal flex layout */
export function Row(props: ContainerProps) {
  const {xstyle, ...rest} = props;
  return <div {...rest} {...stylex.props(styles.flex, styles.row, xstyle)} />;
}

/** Visually empty flex item with `flex-grow: 1` to insert as much space as possible between siblings. */
export function FlexSpacer() {
  return <div {...stylex.props(styles.spacer)} />;
}
