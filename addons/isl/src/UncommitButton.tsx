/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {UncommitOperation} from './operations/Uncommit';
import {latestCommitTreeMap, latestHeadCommit, useRunOperation} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';

export function UncommitButton() {
  // TODO: use treeWithPreviews instead,
  // otherwise there's bugs with disabling this button during previews
  const headCommit = useRecoilValue(latestHeadCommit);
  const treeMap = useRecoilValue(latestCommitTreeMap);
  const runOperation = useRunOperation();
  if (!headCommit) {
    return null;
  }
  const headTree = treeMap.get(headCommit.hash);
  if (!headTree || headTree.children.length) {
    // if the head commit has children, we can't uncommit
    return null;
  }
  return (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      title={t(
        'Remove this commit, but keep its changes as uncommitted changes, as if you never ran commit.',
      )}>
      <VSCodeButton
        onClick={() => runOperation(new UncommitOperation(headCommit))}
        appearance="secondary">
        <T>Uncommit</T>
      </VSCodeButton>
    </Tooltip>
  );
}
