/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DropdownField, DropdownFields} from './DropdownFields';
import {Icon} from './Icon';
import {Tooltip} from './Tooltip';
import {codeReviewProvider, repositoryInfo} from './codeReview/CodeReviewInfo';
import {T} from './i18n';
import {initialParams} from './urlParams';
import {VSCodeBadge, VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {basename} from 'shared/utils';

export function CwdSelector() {
  const info = useRecoilValue(repositoryInfo);
  if (info?.type !== 'success') {
    return null;
  }
  const repoBasename = basename(info.repoRoot);
  return (
    <Tooltip trigger="click" component={CwdSelectorDetails} placement="bottom">
      <VSCodeButton appearance="icon">
        <Icon icon="folder" slot="start" />
        {repoBasename}
      </VSCodeButton>
    </Tooltip>
  );
}

function CwdSelectorDetails() {
  const info = useRecoilValue(repositoryInfo);
  const repoRoot = info?.type === 'success' ? info.repoRoot : null;
  const provider = useRecoilValue(codeReviewProvider);
  return (
    <DropdownFields title={<T>Repository Info</T>} icon="folder">
      <DropdownField title={<T>Repository root</T>}>
        <code>{repoRoot}</code>
      </DropdownField>
      <DropdownField title={<T>Current Working Directory</T>}>
        <code>{initialParams.get('cwd')}</code>
      </DropdownField>
      {provider != null ? (
        <DropdownField title={<T>Code Review Provider</T>}>
          <span>
            <VSCodeBadge>{provider?.name}</VSCodeBadge> <provider.RepoInfo />
          </span>
        </DropdownField>
      ) : null}
    </DropdownFields>
  );
}
