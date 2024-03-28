/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Operation} from './operations/Operation';
import type {CommitInfo, ExactRevset, Hash, SucceedableRevset} from './types';

import {bookmarksDataStorage} from './BookmarksData';
import {Subtle} from './Subtle';
import {tracker} from './analytics';
import {findCurrentPublicBase} from './getCommitTree';
import {T, t} from './i18n';
import {atomFamilyWeak, readAtom} from './jotaiUtils';
import {BulkRebaseOperation} from './operations/BulkRebaseOperation';
import {RebaseAllDraftCommitsOperation} from './operations/RebaseAllDraftCommitsOperation';
import {RebaseOperation} from './operations/RebaseOperation';
import {useRunOperation} from './operationsState';
import {dagWithPreviews} from './previews';
import {RelativeDate} from './relativeDate';
import {commitsShownRange, latestCommits, latestDag} from './serverAPIState';
import {succeedableRevset} from './types';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {atom} from 'jotai';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';

import './SuggestedRebase.css';

/**
 * Whether a given stack (from its base hash) is eligible for currently suggested rebase.
 * Determined by if the stack is old enough and worth rebasing (i.e. not obsolete or closed)
 */
export const showSuggestedRebaseForStack = atomFamilyWeak((hash: Hash) =>
  atom(get => {
    const dag = get(dagWithPreviews);
    const commit = dag.get(hash);
    if (commit == null) {
      return false;
    }
    const parentHash = commit.parents.at(0);
    const stackBase = dag.get(parentHash);
    if (stackBase == null) {
      return false;
    }

    // If the public base is already on a remote bookmark or a stable commit, don't suggest rebasing it.
    if (
      stackBase.remoteBookmarks.length > 0 ||
      stackBase.bookmarks.length > 0 ||
      (stackBase.stableCommitMetadata?.length ?? 0) > 0
    ) {
      return false;
    }

    // If all commits are obsoleted, do not suggest rebasing (but should suggest cleanup).
    const stack = dag.descendants(hash);
    if (stack.size === dag.obsolete(stack).size) {
      return false;
    }

    return true;
  }),
);

export const suggestedRebaseDestinations = atom(get => {
  const dag = get(latestDag);
  const publicBase = findCurrentPublicBase(get(dagWithPreviews));
  const destinations = dag
    .getBatch(dag.public_().toArray())
    .filter(
      commit => commit.remoteBookmarks.length > 0 || (commit.stableCommitMetadata?.length ?? 0) > 0,
    )
    .map((commit): [CommitInfo, string] => [
      commit,
      [
        ...commit.remoteBookmarks,
        ...(commit.stableCommitMetadata?.map(s => s.value) ?? []),
        ...commit.bookmarks,
      ].join(', '),
    ])
    .filter(([_commit, label]) => label.length > 0);
  if (publicBase) {
    const publicBaseLabel = t('Current Stack Base');
    const existing = destinations.find(dest => dest[0].hash === publicBase.hash);
    if (existing != null) {
      existing[1] = [publicBaseLabel, existing[1]].join(', ');
    } else {
      destinations.push([publicBase, publicBaseLabel]);
    }
  }
  destinations.sort((a, b) => b[0].date.valueOf() - a[0].date.valueOf());

  return destinations;
});

export function SuggestedRebaseButton({
  source,
  sources,
  afterRun,
}:
  | {
      source: SucceedableRevset | ExactRevset;
      sources?: undefined;
      afterRun?: () => unknown;
    }
  | {
      source?: undefined;
      sources: Array<SucceedableRevset>;
      afterRun?: () => unknown;
    }
  | {
      source?: undefined;
      sources?: undefined;
      afterRun?: () => unknown;
    }) {
  const runOperation = useRunOperation();
  const isBulk = source == null;
  const isAllDraftCommits = sources == null && source == null;
  const showContextMenu = useContextMenu(() => {
    const destinations = readAtom(suggestedRebaseDestinations);
    return (
      destinations?.map(([dest, label]) => {
        return {
          label: (
            <span className="suggested-rebase-context-menu-option">
              <span>{label}</span>
              <Subtle>
                <RelativeDate date={dest.date} />
              </Subtle>
            </span>
          ),
          onClick: () => {
            runOperation(getSuggestedRebaseOperation(dest, source ?? sources));
            afterRun?.();
          },
        };
      }) ?? []
    );
  });
  return (
    <VSCodeButton appearance={isBulk ? 'secondary' : 'icon'} onClick={showContextMenu}>
      <Icon icon="git-pull-request" slot="start" />
      {isAllDraftCommits ? (
        <T>Rebase all onto&hellip;</T>
      ) : isBulk ? (
        <T>Rebase selected commits onto...</T>
      ) : (
        <T>Rebase onto&hellip;</T>
      )}
    </VSCodeButton>
  );
}

/**
 * Returns an operation that will rebase the given source onto the given destination.
 * If source is undefined, rebase all draft commits.
 * If source is an Array of revsets, bulk rebase those commits.
 * If source is a single revset, rebase that commit.
 */
export function getSuggestedRebaseOperation(
  dest: CommitInfo,
  source: SucceedableRevset | ExactRevset | Array<SucceedableRevset> | undefined,
): Operation {
  const destination = dest.remoteBookmarks?.[0] ?? dest.hash;
  const isBulk = source != null && Array.isArray(source);
  tracker.track('ClickSuggestedRebase', {
    extras: {destination, isBulk, locations: dest.stableCommitMetadata?.map(s => s.value)},
  });

  const operation =
    source == null
      ? new RebaseAllDraftCommitsOperation(
          readAtom(commitsShownRange),
          succeedableRevset(destination),
        )
      : Array.isArray(source)
      ? new BulkRebaseOperation(source, succeedableRevset(destination))
      : new RebaseOperation(source, succeedableRevset(destination));

  return operation;
}
