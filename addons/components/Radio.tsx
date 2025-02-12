/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type react from 'react';

import * as stylex from '@stylexjs/stylex';
import {useId} from 'react';
import {Column} from './Flex';
import {Tooltip} from './Tooltip';
import {layout} from './theme/layout';
import {spacing} from './theme/tokens.stylex';

// stylex doesn't support :checked and :before simultaneously very well
import './Radio.css';

const styles = stylex.create({
  group: {
    appearance: 'none',
    border: 'none',
    boxSizing: 'border-box',
    alignItems: 'flex-start',
    marginInline: 0,
    marginBlock: spacing.pad,
    padding: 0,
  },
  label: {
    cursor: 'pointer',
  },
  horizontal: {
    flexDirection: 'row',
    flexWrap: 'wrap',
  },
  disabled: {
    opacity: 0.5,
    cursor: 'not-allowed',
  },
});

export function RadioGroup<T extends string>({
  title,
  choices,
  current,
  onChange,
  horizontal,
}: {
  title?: string;
  choices: Array<{value: T; title: react.ReactNode; tooltip?: string; disabled?: boolean}>;
  current: T;
  onChange: (t: T) => unknown;
  horizontal?: boolean;
}) {
  const inner = (
    <fieldset
      {...stylex.props(layout.flexCol, styles.group, horizontal === true && styles.horizontal)}>
      {choices.map(({value, title, tooltip, disabled}) => (
        <Radio
          key={value}
          value={value}
          title={title}
          tooltip={tooltip}
          disabled={disabled}
          checked={current === value}
          onChange={() => onChange(value)}
        />
      ))}
    </fieldset>
  );
  return title == null ? (
    inner
  ) : (
    <Column alignStart>
      <strong>{title}</strong>
      {inner}
    </Column>
  );
}

function Radio({
  title,
  value,
  tooltip,
  checked,
  onChange,
  disabled,
}: {
  title: react.ReactNode;
  value: string;
  tooltip?: string;
  checked: boolean;
  onChange: () => unknown;
  disabled?: boolean;
}) {
  const id = useId();
  const inner = (
    <label
      htmlFor={id}
      {...stylex.props(layout.flexRow, styles.label, disabled && styles.disabled)}>
      <input
        type="radio"
        id={id}
        name={value}
        value={value}
        checked={checked}
        onChange={onChange}
        className="isl-radio"
        disabled={disabled}
      />
      {title}
    </label>
  );
  return tooltip ? <Tooltip title={tooltip}>{inner}</Tooltip> : inner;
}
