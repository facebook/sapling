/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DropdownFields} from './DropdownFields';
import {Internal} from './Internal';
import {Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {PullRevOperation} from './operations/PullRevOperation';
import {useRunOperation} from './serverAPIState';
import {VSCodeButton, VSCodeTextField} from '@vscode/webview-ui-toolkit/react';
import {useState} from 'react';
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
  const [enteredDiffNum, setEnteredDiffNum] = useState('');
  const runOperation = useRunOperation();
  const supportsDiffDownload = Internal.diffDownloadOperation != null;
  return (
    <DropdownFields
      title={<T>Download Commits</T>}
      icon="cloud-download"
      data-testid="settings-dropdown">
      <div className="download-commits-input-row">
        <VSCodeTextField
          placeholder={supportsDiffDownload ? t('Hash, Diff Number, ...') : t('Hash, revset, ...')}
          value={enteredDiffNum}
          onChange={e => setEnteredDiffNum((e.target as unknown as {value: string})?.value ?? '')}
        />
        <VSCodeButton
          appearance="secondary"
          data-testid="download-commit-button"
          onClick={() => {
            if (Internal.diffDownloadOperation != null) {
              runOperation(Internal.diffDownloadOperation(enteredDiffNum));
            } else {
              runOperation(new PullRevOperation(enteredDiffNum));
            }
          }}>
          <T>Pull</T>
        </VSCodeButton>
      </div>
    </DropdownFields>
  );
}
