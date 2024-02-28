/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type react from 'react';

import {Column} from '../ComponentUtils';
import {layout} from '../stylexUtils';
import {spacing} from '../tokens.stylex';
import * as stylex from '@stylexjs/stylex';
import {useId} from 'react';

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
});

export function RadioGroup<T extends string>({
  title,
  choices,
  current,
  onChange,
}: {
  title?: string;
  choices: Array<{value: T; title: react.ReactNode}>;
  current: T;
  onChange: (t: T) => unknown;
}) {
  return (
    <Column>
      <strong>{title}</strong>
      <fieldset {...stylex.props(layout.flexCol, styles.group)}>
        {choices.map(({value, title}) => (
          <Radio
            key={value}
            value={value}
            title={title}
            checked={current === value}
            onChange={() => onChange(value)}
          />
        ))}
      </fieldset>
    </Column>
  );
}

function Radio({
  title,
  value,
  checked,
  onChange,
}: {
  title: react.ReactNode;
  value: string;
  checked: boolean;
  onChange: () => unknown;
}) {
  const id = useId();
  return (
    <label htmlFor={id} {...stylex.props(layout.flexRow, styles.label)}>
      <input
        type="radio"
        id={id}
        name={value}
        value={value}
        checked={checked}
        onChange={onChange}
        className="isl-radio"
      />
      {title}
    </label>
  );
}
