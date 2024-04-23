/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {Button} from './components/Button';
import * as stylex from '@stylexjs/stylex';
import {Icon} from 'shared/Icon';

import './VSCodeButtonDropdown.css';

const styles = stylex.create({
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
    cursor: {
      default: 'pointer',
      ':disabled': 'not-allowed',
    },
    width: '24px',
    borderRadius: '0px 2px 2px 0px; /* meet with button *',
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
}: {
  options: ReadonlyArray<T>;
  kind?: 'primary' | undefined; // icon-type buttons don't have a natrual spot for the dropdown
  onClick: (selected: T) => unknown;
  selected: T;
  onChangeSelected: (newSelected: T) => unknown;
  buttonDisabled?: boolean;
  pickerDisabled?: boolean;
  /** Icon to place in the button */
  icon?: React.ReactNode;
}) {
  const selectedOption = options.find(opt => opt.id === selected.id) ?? options[0];
  return (
    <div {...stylex.props(styles.container)}>
      <Button
        kind={kind}
        onClick={buttonDisabled ? undefined : () => onClick(selected)}
        disabled={buttonDisabled}
        xstyle={styles.button}>
        {icon ?? null} {selectedOption.label}
      </Button>
      <select
        {...stylex.props(styles.select)}
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
      <Icon icon="chevron-down" {...stylex.props(styles.chevron)} />
    </div>
  );
}
