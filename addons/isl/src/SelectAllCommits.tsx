/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {KeyCode, Modifier} from 'isl-components/KeyboardShortcuts';
import {Tooltip} from 'isl-components/Tooltip';
import {useCallback} from 'react';
import {useCommand} from './ISLShortcuts';
import {islDrawerState} from './drawerState';
import {t, T} from './i18n';
import {readAtom, writeAtom} from './jotaiUtils';
import {dagWithPreviews} from './previews';
import {selectedCommits} from './selection';

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
      <Button
        data-testid="select-all-button"
        onClick={() => {
          onClick();
          dismiss();
        }}>
        <Icon icon="check-all" slot="start" />
        <T>Select all commits</T>
        <Kbd keycode={KeyCode.A} modifiers={[Modifier.ALT]} />
      </Button>
    </Tooltip>
  );
}
