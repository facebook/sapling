/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useCommand} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {Tooltip} from './Tooltip';
import {islDrawerState} from './drawerState';
import {t, T} from './i18n';
import {readAtom, writeAtom} from './jotaiUtils';
import {dagWithPreviews} from './previews';
import {selectedCommits} from './selection';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useCallback} from 'react';
import {Icon} from 'shared/Icon';
import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';

/** By default, "select all" selects draft, non-obsoleted commits. */
function getSelectAllCommitHashSet(): Set<string> {
  const dag = readAtom(dagWithPreviews);
  return new Set(
    dag
      .nonObsolete(dag.draft())
      .toArray()
      .filter(hash => !hash.startsWith('OPTIMISTIC')),
  );
}

export function useSelectAllCommitsShortcut() {
  const cb = useSelectAllCommits();
  useCommand('SelectAllCommits', cb);
}

export function useSelectAllCommits() {
  return useCallback(() => {
    const draftCommits = getSelectAllCommitHashSet();
    writeAtom(selectedCommits, draftCommits);
    // pop open sidebar so you can act on the bulk selection
    writeAtom(islDrawerState, last => ({
      ...last,
      right: {
        ...last.right,
        collapsed: false,
      },
    }));
  }, []);
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
