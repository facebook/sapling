/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {DropdownFields} from './DropdownFields';
import {T} from './i18n';
import type {CommitInfo} from './types';

import './SmartActionsMenu.css';
import {BaseSplitButton} from './stackEdit/ui/BaseSplitButton';

export function SmartActionsMenu({commit}: {commit: CommitInfo}) {
  return (
    <Tooltip
      component={dismiss => <SmartActions commit={commit} dismiss={dismiss} />}
      trigger="click"
      title={<T>Smart Actions...</T>}>
      <Button icon data-testid="smart-actions-button" className="smart-actions-button">
        <Icon icon="lightbulb" />
      </Button>
    </Tooltip>
  );
}

function SmartActions({commit, dismiss}: {commit: CommitInfo; dismiss: () => void}) {
  return (
    <DropdownFields
      title={<T>Smart Actions</T>}
      icon="lightbulb"
      className="smart-actions-dropdown"
      data-testid="smart-actions-dropdown">
      <AutoSplitButton commit={commit} dismiss={dismiss} />
    </DropdownFields>
  );
}

/** Like SplitButton, but triggers AI split automatically. */
function AutoSplitButton({commit, dismiss}: {commit: CommitInfo; dismiss: () => void}) {
  return (
    <BaseSplitButton
      commit={commit}
      trackerEventName="SplitOpenFromSmartActions"
      autoSplit={true}
      onSplitInitiated={dismiss}>
      <T>Auto-split with AI</T>
    </BaseSplitButton>
  );
}
