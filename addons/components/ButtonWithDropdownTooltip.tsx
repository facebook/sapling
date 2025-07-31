/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {type ForwardedRef, forwardRef, type ReactNode} from 'react';

import * as stylex from '@stylexjs/stylex';
import {Button} from './Button';
import {Icon} from './Icon';
import {colors, spacing} from './theme/tokens.stylex';
import {Tooltip} from './Tooltip';

const styles = stylex.create({
  container: {
    display: 'flex',
    alignItems: 'stretch',
    position: 'relative',
  },
  button: {
    borderBottomRightRadius: 0,
    borderTopRightRadius: 0,
  },
  chevron: {
    borderBottomLeftRadius: 0,
    borderTopLeftRadius: 0,
    borderLeft: 'unset',
    width: '24px',
    height: '24px',
    paddingTop: 6,
  },
  builtinButtonBorder: {
    borderLeft: 'unset',
  },
  iconButton: {
    borderTopRightRadius: 0,
    borderBottomRightRadius: 0,
    paddingRight: spacing.half,
  },
  iconSelect: {
    borderTopLeftRadius: 0,
    borderBottomLeftRadius: 0,
    borderLeftColor: colors.hoverDarken,
  },
  chevronDisabled: {
    opacity: 0.5,
  },
});

export const ButtonWithDropdownTooltip = forwardRef(
  (
    {
      label,
      kind,
      onClick,
      disabled,
      icon,
      tooltip,
      ...rest
    }: {
      label: ReactNode;
      kind?: 'primary' | 'icon' | undefined;
      onClick: () => unknown;
      disabled?: boolean;
      icon?: React.ReactNode;
      tooltip: React.ReactNode;
      'data-testId'?: string;
    },
    ref: ForwardedRef<HTMLButtonElement>,
  ) => {
    return (
      <div {...stylex.props(styles.container)}>
        <Button
          kind={kind}
          onClick={disabled ? undefined : () => onClick()}
          disabled={disabled}
          xstyle={[styles.button, kind === 'icon' && styles.iconButton]}
          ref={ref}
          {...rest}>
          {icon ?? null} {label}
        </Button>
        <Tooltip
          trigger="click"
          component={_dismiss => <div>{tooltip}</div>}
          group="topbar"
          placement="bottom">
          <Button
            kind={kind}
            onClick={undefined}
            disabled={disabled}
            xstyle={[styles.chevron]}
            {...rest}>
            <Icon
              icon="chevron-down"
              {...stylex.props(styles.chevron, disabled && styles.chevronDisabled)}
            />
          </Button>
        </Tooltip>
      </div>
    );
  },
);
