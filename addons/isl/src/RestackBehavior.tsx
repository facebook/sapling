/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {persistAtomToLocalStorageEffect} from './persistAtomToConfigEffect';
import {VSCodeDropdown, VSCodeOption} from '@vscode/webview-ui-toolkit/react';
import {atom, useRecoilState} from 'recoil';

export enum AmendRestackBehavior {
  ALWAYS = 'always',
  NEVER = 'never',
  NO_CONFLICT = 'no-conflict',
}
const DEFAULT_AMEND_RESTACK_BEHAVIOR = AmendRestackBehavior.ALWAYS;

/** This is controleld by the underlying Sapling config for this feature.
 * This way, we don't pass any additional data to sl to run amend,
 * and this one setting controls this behavior everywhere.
 * We merely give the setting a UI since it's common to customize.
 */
export const restackBehaviorAtom = atom<AmendRestackBehavior>({
  key: 'restackBehaviorAtom',
  default: DEFAULT_AMEND_RESTACK_BEHAVIOR,
  effects: [persistAtomToLocalStorageEffect<AmendRestackBehavior>('isl.amend-autorestack')],
});

export function RestackBehaviorSetting() {
  const [value, setValue] = useRecoilState(restackBehaviorAtom);
  return (
    <Tooltip
      title={t(
        'Whether to restack (rebase) child commits when amending a commit in a stack. ' +
          'By default, commits are always restacked, even if it introduces merge conflicts. ',
      )}>
      <div className="dropdown-container setting-inline-dropdown">
        <label htmlFor="restack-setting">
          <T>Restack on Amend</T>
        </label>
        <VSCodeDropdown
          value={value}
          id="restack-setting"
          onChange={event =>
            setValue(
              (event as React.FormEvent<HTMLSelectElement>).currentTarget
                .value as AmendRestackBehavior,
            )
          }>
          <VSCodeOption value={AmendRestackBehavior.NO_CONFLICT}>
            <T>No Conflict</T>
          </VSCodeOption>
          <VSCodeOption value={AmendRestackBehavior.ALWAYS}>
            <T>Always</T>
          </VSCodeOption>
          <VSCodeOption value={AmendRestackBehavior.NEVER}>
            <T>Never</T>
          </VSCodeOption>
        </VSCodeDropdown>
      </div>
    </Tooltip>
  );
}
