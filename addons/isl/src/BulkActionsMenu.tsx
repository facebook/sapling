/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {CleanupAllButton} from './Cleanup';
import {DropdownFields} from './DropdownFields';
import {SelectAllButton} from './SelectAllCommits';
import {SuggestedRebaseButton} from './SuggestedRebase';
import {Tooltip} from './Tooltip';
import {T} from './i18n';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';

import './BulkActionsMenu.css';

export function BulkActionsMenu() {
  return (
    <Tooltip
      component={dismiss => <BulkActions dismiss={dismiss} />}
      trigger="click"
      placement="bottom"
      title={<T>Bulk Actions</T>}>
      <VSCodeButton appearance="icon" data-testid="bulk-actions-button">
        <Icon icon="run-all" />
      </VSCodeButton>
    </Tooltip>
  );
}

function BulkActions({dismiss}: {dismiss: () => void}) {
  return (
    <DropdownFields
      title={<T>Bulk Actions</T>}
      icon="run-all"
      className="bulk-actions-dropdown"
      data-testid="bulk-actions-dropdown">
      <SelectAllButton dismiss={dismiss} />
      <SuggestedRebaseButton afterRun={dismiss} />
      <CleanupAllButton />
    </DropdownFields>
  );
}
