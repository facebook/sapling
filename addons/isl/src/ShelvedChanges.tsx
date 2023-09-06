/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DropdownFields} from './DropdownFields';
import {Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';

export function ShelvedChangesMenu() {
  return (
    <Tooltip
      component={ShelvedChangesList}
      trigger="click"
      placement="bottom"
      title={t('Shelved Changes')}>
      <VSCodeButton appearance="icon" data-testid="shelved-changes-button">
        <Icon icon="archive" />
      </VSCodeButton>
    </Tooltip>
  );
}

export function ShelvedChangesList() {
  return (
    <DropdownFields
      title={<T>Shelved Changes</T>}
      icon="archive"
      data-testid="shelved-changes-dropdown">
      <div className="">default</div>
    </DropdownFields>
  );
}
