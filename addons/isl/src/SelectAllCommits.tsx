/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Snapshot} from 'recoil';

import {useCommand} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {Tooltip} from './Tooltip';
import {islDrawerState} from './drawerState';
import {t, T} from './i18n';
import {linearizedCommitHistory, selectedCommits} from './selection';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilCallback} from 'recoil';
import {Icon} from 'shared/Icon';
import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';

export function getAllDraftCommits(snapshot: Snapshot): Array<string> {
  const commits = snapshot.getLoadable(linearizedCommitHistory).valueMaybe();
  if (commits == null) {
    return [];
  }
  const draftCommits = commits
    .filter(commit => commit.phase !== 'public' && !commit.hash.startsWith('OPTIMISTIC'))
    .map(commit => commit.hash);
  draftCommits.reverse();
  return draftCommits;
}

export function useSelectAllCommitsShortcut() {
  const cb = useSelectAllCommits();
  useCommand('SelectAllCommits', cb);
}

export function useSelectAllCommits() {
  return useRecoilCallback(({set, snapshot}) => () => {
    const draftCommits = getAllDraftCommits(snapshot);
    set(selectedCommits, new Set(draftCommits));
    // pop open sidebar so you can act on the bulk selection
    set(islDrawerState, last => ({
      ...last,
      right: {
        ...last.right,
        collapsed: false,
      },
    }));
  });
}

export function SelectAllButton({dismiss}: {dismiss: () => unknown}) {
  const onClick = useSelectAllCommits();
  return (
    <Tooltip
      title={t(
        'Select all draft commits. This allows more granular bulk manipulations in the sidebar.',
      )}>
      <VSCodeButton
        appearance="secondary"
        data-testid="select-all-button"
        onClick={() => {
          onClick();
          dismiss();
        }}>
        <Icon icon="check-all" slot="start" />
        <T>Select all commits</T> <Icon icon="chevron-right" />
        <Kbd keycode={KeyCode.A} modifiers={[Modifier.ALT]} />
      </VSCodeButton>
    </Tooltip>
  );
}
