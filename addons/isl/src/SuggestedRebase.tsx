/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash} from './types';

import {Subtle} from './Subtle';
import {tracker} from './analytics';
import {findCurrentPublicBase} from './getCommitTree';
import {T, t} from './i18n';
import {RebaseOperation} from './operations/RebaseOperation';
import {treeWithPreviews} from './previews';
import {RelativeDate} from './relativeDate';
import {latestCommits, useRunOperation} from './serverAPIState';
import {succeedableRevset} from './types';
import {short} from './utils';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {selector, selectorFamily, useRecoilCallback} from 'recoil';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';

import './SuggestedRebase.css';

/**
 * Whether a given stack (from its base hash) is eligible for currently suggested rebase.
 * Determined by if the stack is old enough and worth rebasing (i.e. not obsolete or closed)
 */
export const showSuggestedRebaseForStack = selectorFamily<boolean, Hash>({
  key: 'showSuggestedRebaseForStack',
  get:
    (hash: Hash) =>
    ({get}) => {
      const tree = get(treeWithPreviews);
      const commit = tree.treeMap.get(hash);
      if (commit == null) {
        return false;
      }
      const parentHash = commit.info.parents[0];
      const stackBase = tree.treeMap.get(parentHash)?.info;
      if (stackBase == null) {
        return false;
      }

      // If the public base is already on a remote bookmark or a stable commit, don't suggest rebasing it.
      return (
        stackBase.remoteBookmarks.length === 0 &&
        stackBase.bookmarks.length === 0 &&
        (stackBase.stableCommitMetadata?.length ?? 0) === 0
      );
    },
});

export const suggestedRebaseDestinations = selector<Array<[CommitInfo, string]>>({
  key: 'suggestedRebaseDestinations',
  get: ({get}) => {
    const commits = get(latestCommits);
    const publicBase = findCurrentPublicBase(get(treeWithPreviews));
    const destinations = commits
      .filter(
        commit =>
          commit.remoteBookmarks.length > 0 || (commit.stableCommitMetadata?.length ?? 0) > 0,
      )
      .map((commit): [CommitInfo, string] => [
        commit,
        firstNonEmptySublist(
          commit.remoteBookmarks,
          commit.stableCommitMetadata?.map(s => s.value),
          commit.bookmarks,
        ) || short(commit.hash),
      ]);
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

    // TODO: we could make the current stack you're based on an option here,
    // to allow rebasing other stacks to the same place as your current stack.
    // Might be unnecessary given we support stableCommitMetadata.

    return destinations;
  },
});

export function SuggestedRebaseButton({stackBaseHash}: {stackBaseHash: Hash}) {
  const validDestinations = useRecoilCallback(({snapshot}) => () => {
    return snapshot.getLoadable(suggestedRebaseDestinations).valueMaybe();
  });
  const runOperation = useRunOperation();
  const showContextMenu = useContextMenu(() => {
    const destinations = validDestinations();
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
            const destination = dest.remoteBookmarks?.[0] ?? dest.hash;
            tracker.track('ClickSuggestedRebase', {extras: {destination}});
            runOperation(
              new RebaseOperation(succeedableRevset(stackBaseHash), succeedableRevset(destination)),
            );
          },
        };
      }) ?? []
    );
  });
  return (
    <VSCodeButton appearance="icon" onClick={showContextMenu}>
      <Icon icon="git-pull-request" slot="start" />
      <T>Rebase onto&hellip;</T>
    </VSCodeButton>
  );
}

function firstNonEmptySublist(...lists: Array<Array<string> | undefined>) {
  for (const list of lists) {
    if (list != null && list.length > 0) {
      return list.join(', ');
    }
  }
}
