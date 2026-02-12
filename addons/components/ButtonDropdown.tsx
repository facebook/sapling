/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import * as stylex from '@stylexjs/stylex';
import {Button, buttonStyles} from './Button';
import {Icon} from './Icon';
import {colors, spacing} from './theme/tokens.stylex';
import {Tooltip, type TooltipProps} from './Tooltip';

export const styles = stylex.create({
  container: {
    display: 'flex',
    alignItems: 'stretch',
    position: 'relative',
    '::before': {
      content: '',
      position: 'absolute',
      width: '100%',
      height: '100%',
      top: 0,
      left: 0,
      pointerEvents: 'none',
    },
  },
  button: {
    borderBottomRightRadius: 0,
    borderTopRightRadius: 0,
  },
  chevron: {
    color: 'var(--button-secondary-foreground)',
    position: 'absolute',
    top: 'calc(50% - 0.5em)',
    right: 'calc(var(--halfpad) - 1px)',
    pointerEvents: 'none',
  },
  select: {
    backgroundColor: {
      default: 'var(--button-secondary-background)',
      ':hover': 'var(--button-secondary-hover-background)',
    },
    color: 'var(--button-secondary-foreground)',
    opacity: {
      default: 1,
      ':disabled': 0.5,
    },
    cursor: {
      default: 'pointer',
      ':disabled': 'not-allowed',
    },
    width: '24px',
    borderRadius: '0px 2px 2px 0px',
    outline: {
      default: 'none',
      ':focus': '1px solid var(--focus-border)',
    },
    outlineOffset: '2px',
    verticalAlign: 'bottom',
    appearance: 'none',
    lineHeight: '0',
    border: '1px solid var(--button-border)',
    borderLeft: '1px solid var(--button-secondary-foreground)',
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
  iconChevron: {
    color: 'var(--button-icon-foreground)',
  },
  chevronDisabled: {
    opacity: 0.5,
  },
});

export function ButtonDropdown<T extends {label: ReactNode; id: string}>({
  options,
  kind,
  onClick,
  selected,
  onChangeSelected,
  buttonDisabled,
  pickerDisabled,
  icon,
  customSelectComponent,
  primaryTooltip,
  ...rest
}: {
  options: ReadonlyArray<T>;
  kind?: 'primary' | 'icon' | undefined;
  onClick: (selected: T, event: React.MouseEvent<HTMLButtonElement>) => unknown;
  selected: T;
  onChangeSelected: (newSelected: T) => unknown;
  buttonDisabled?: boolean;
  pickerDisabled?: boolean;
  /** Icon to place in the button */
  icon?: React.ReactNode;
  customSelectComponent?: React.ReactNode;
  primaryTooltip?: TooltipProps;
  'data-testId'?: string;
}) {
  const selectedOption = options.find(opt => opt.id === selected.id) ?? options[0];
  // const themeName = useAtomValue(themeNameState); // TODO
  // // Slightly hacky: in these themes, the border is too strong. Use the button border instead.
  // const useBuiltinBorder = ['Default Light Modern', 'Default Dark Modern'].includes(
  //   themeName as string,
  // );

  const buttonComponent = (
    <Button
      kind={kind}
      onClick={buttonDisabled ? undefined : e => onClick(selected, e)}
      disabled={buttonDisabled}
      xstyle={[styles.button, kind === 'icon' && styles.iconButton]}
      {...rest}>
      {icon ?? null} {selected.label}
    </Button>
  );

  return (
    <div {...stylex.props(styles.container)}>
      {primaryTooltip ? <Tooltip {...primaryTooltip}>{buttonComponent}</Tooltip> : buttonComponent}
      {customSelectComponent ?? (
        <select
          {...stylex.props(
            styles.select,
            kind === 'icon' && buttonStyles.icon,
            kind === 'icon' && styles.iconSelect,
            // useBuiltinBorder && styles.builtinButtonBorder,
          )}
          disabled={pickerDisabled}
          value={selectedOption.id}
          onClick={e => e.stopPropagation()}
          onChange={event => {
            const matching = options.find(opt => opt.id === (event.target.value as T['id']));
            if (matching != null) {
              onChangeSelected(matching);
            }
          }}>
          {options.map(option => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </select>
      )}
      <Icon
        icon="chevron-down"
        {...stylex.props(
          styles.chevron,
          kind === 'icon' && styles.iconChevron,
          pickerDisabled && styles.chevronDisabled,
        )}
      />
    </div>
  );
}
