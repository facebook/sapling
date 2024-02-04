/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ButtonAppearance} from '@vscode/webview-ui-toolkit';
import type {ReactNode} from 'react';

import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';

import './VSCodeButtonDropdown.css';

export function VSCodeButtonDropdown<T extends {label: ReactNode; id: string}>({
  options,
  appearance,
  onClick,
  selected,
  onChangeSelected,
  buttonDisabled,
  pickerDisabled,
  icon,
}: {
  options: ReadonlyArray<T>;
  appearance: Exclude<ButtonAppearance, 'icon'>; // icon-type buttons don't have a natrual spot for the dropdown
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
    <div className="vscode-button-dropdown">
      <VSCodeButton
        appearance={appearance}
        onClick={buttonDisabled ? undefined : () => onClick(selected)}
        disabled={buttonDisabled}>
        {icon ?? null} {selectedOption.label}
      </VSCodeButton>
      <select
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
      <Icon icon="chevron-down" />
    </div>
  );
}
