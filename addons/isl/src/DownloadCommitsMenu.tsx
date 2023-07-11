/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DropdownFields} from './DropdownFields';
import {Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {VSCodeButton, VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';

import './DownloadCommitsMenu.css';

export function DownloadCommitsTooltipButton() {
  return (
    <Tooltip
      trigger="click"
      component={() => <DownloadCommitsTooltip />}
      placement="bottom"
      title={t('Download commits')}>
      <VSCodeButton appearance="icon" data-testid="download-commits-tooltip-button">
        <Icon icon="cloud-download" />
      </VSCodeButton>
    </Tooltip>
  );
}

function DownloadCommitsTooltip() {
  return (
    <DropdownFields
      title={<T>Download Commits</T>}
      icon="cloud-download"
      data-testid="settings-dropdown">
      <div className="download-commits-input-row">
        <VSCodeTextField placeholder={t('Hash, Diff Number, ...')} />
        <VSCodeButton appearance="secondary" data-testid="download-commit-button">
          <T>Pull</T>
        </VSCodeButton>
      </div>
    </DropdownFields>
  );
}
