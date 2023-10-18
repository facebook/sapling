/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Icon} from 'shared/Icon';

import './CommitCloud.css';

export function CommitCloudInfo() {
  return (
    <div className="commit-cloud-info">
      <div className="dropdown-fields-header commit-cloud-header">
        <Icon icon="cloud" size="M" />
        <strong role="heading">{<T>Commit Cloud</T>}</strong>
        <Tooltip
          title={t(
            'Commit Cloud backs up your draft commits automatically across all your devices.',
          )}>
          <Icon icon="info" />
        </Tooltip>
      </div>
      <div className="download-commits-input-row">
        <T replace={{$relTimeAgo: '2 minutes ago'}}>Last backed up $relTimeAgo</T>
        <VSCodeButton appearance="secondary">
          <T>Sync now</T>
        </VSCodeButton>
      </div>
    </div>
  );
}
