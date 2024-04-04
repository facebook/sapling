/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, DiffSummary, Hash, PageVisibility, RepoInfo, Result} from '../types';
import type {UICodeReviewProvider} from './UICodeReviewProvider';

import serverAPI from '../ClientToServerAPI';
import {commitMessageTemplate} from '../CommitInfoView/CommitInfoState';
import {
  applyEditedFields,
  commitMessageFieldsSchema,
  commitMessageFieldsToString,
  emptyCommitMessageFields,
  parseCommitMessageFields,
} from '../CommitInfoView/CommitMessageFields';
import {Internal} from '../Internal';
import {atomFamilyWeak, atomWithOnChange, writeAtom} from '../jotaiUtils';
import {messageSyncingEnabledState} from '../messageSyncing';
import {dagWithPreviews} from '../previews';
import {commitByHash, repositoryInfo} from '../serverAPIState';
import {registerCleanup, registerDisposable} from '../utils';
import {GithubUICodeReviewProvider} from './github/github';
import {atom} from 'jotai';
import {clearTrackedCache} from 'shared/LRU';
import {debounce} from 'shared/debounce';
import {firstLine, nullthrows} from 'shared/utils';

export const codeReviewProvider = atom<UICodeReviewProvider | null>(get => {
  const repoInfo = get(repositoryInfo);
  return repoInfoToCodeReviewProvider(repoInfo);
});

function repoInfoToCodeReviewProvider(repoInfo?: RepoInfo): UICodeReviewProvider | null {
  if (repoInfo?.type !== 'success') {
    return null;
  }
  if (repoInfo.codeReviewSystem.type === 'github') {
    return new GithubUICodeReviewProvider(
      repoInfo.codeReviewSystem,
      repoInfo.preferredSubmitCommand ?? 'pr',
    );
  }
  if (
    repoInfo.codeReviewSystem.type === 'phabricator' &&
    Internal.PhabricatorUICodeReviewProvider != null
  ) {
    return new Internal.PhabricatorUICodeReviewProvider(repoInfo.codeReviewSystem);
  }
  return null;
}

export const diffSummary = atomFamilyWeak((diffId: DiffId | undefined) =>
  atom<Result<DiffSummary | undefined>>(get => {
    if (diffId == null) {
      return {value: undefined};
    }
    const all = get(allDiffSummaries);
    if (all == null) {
      return {value: undefined};
    }
    if (all.error) {
      return {error: all.error};
    }
    return {value: all.value?.get(diffId)};
  }),
);

export const allDiffSummaries = atom<Result<Map<DiffId, DiffSummary> | null>>({value: null});

registerDisposable(
  allDiffSummaries,
  serverAPI.onMessageOfType('fetchedDiffSummaries', event => {
    writeAtom(allDiffSummaries, existing => {
      if (existing.error) {
        // TODO: if we only fetch one diff, but had an error on the overall fetch... should we still somehow show that error...?
        // Right now, this will reset all other diffs to "loading" instead of error
        // Probably, if all diffs fail to fetch, so will individual diffs.
        return event.summaries;
      }

      if (event.summaries.error || existing.value == null) {
        return event.summaries;
      }

      // merge old values with newly fetched ones
      return {
        value: new Map([
          ...nullthrows(existing.value).entries(),
          ...event.summaries.value.entries(),
        ]),
      };
    });
  }),
  import.meta.hot,
);

registerCleanup(
  allDiffSummaries,
  serverAPI.onSetup(() =>
    serverAPI.postMessage({
      type: 'fetchDiffSummaries',
    }),
  ),
  import.meta.hot,
);

/**
 * Latest commit message (title,description) for a hash.
 * There's multiple competing values, in order of priority:
 * (1) the optimistic commit's message
 * (2) the latest commit message on the server (phabricator/github)
 * (3) the local commit's message
 *
 * Remote messages preferred above local messages, so you see remote changes accounted for.
 * Optimistic changes preferred above remote changes, since we should always
 * async update the remote message to match the optimistic state anyway, but the UI will
 * be smoother if we use the optimistic one before the remote has gotten the update propagated.
 * This is only necessary if the optimistic message is different than the local message.
 */
export const latestCommitMessage = atomFamilyWeak((hash: Hash | 'head') =>
  atom((get): [title: string, description: string] => {
    if (hash === 'head') {
      const template = get(commitMessageTemplate);
      if (template) {
        const schema = get(commitMessageFieldsSchema);
        const result = applyEditedFields(emptyCommitMessageFields(schema), template);
        const templateString = commitMessageFieldsToString(
          schema,
          result,
          /* allowEmptyTitle */ true,
        );
        const title = firstLine(templateString);
        const description = templateString.slice(title.length);
        return [title, description];
      }
      return ['', ''];
    }
    const commit = get(commitByHash(hash));
    const preview = get(dagWithPreviews).get(hash);

    if (
      preview != null &&
      (preview.title !== commit?.title || preview.description !== commit?.description)
    ) {
      return [preview.title, preview.description];
    }

    if (!commit) {
      return ['', ''];
    }

    const syncEnabled = get(messageSyncingEnabledState);

    let remoteTitle = commit.title;
    let remoteDescription = commit.description;
    if (syncEnabled && commit.diffId) {
      // use the diff's commit message instead of the local one, if available
      const summary = get(diffSummary(commit.diffId));
      if (summary?.value) {
        remoteTitle = summary.value.title;
        remoteDescription = summary.value.commitMessage;
      }
    }

    return [remoteTitle, remoteDescription];
  }),
);

export const latestCommitMessageTitle = atomFamilyWeak((hashOrHead: Hash | 'head') =>
  atom(get => {
    const [title] = get(latestCommitMessage(hashOrHead));
    return title;
  }),
);

export const latestCommitMessageFields = atomFamilyWeak((hashOrHead: Hash | 'head') =>
  atom(get => {
    const [title, description] = get(latestCommitMessage(hashOrHead));
    const schema = get(commitMessageFieldsSchema);
    return parseCommitMessageFields(schema, title, description);
  }),
);

export const pageVisibility = atomWithOnChange(
  atom<PageVisibility>(document.hasFocus() ? 'focused' : document.visibilityState),
  debounce(state => {
    serverAPI.postMessage({
      type: 'pageVisibility',
      state,
    });
  }, 50),
);

const handleVisibilityChange = () => {
  const newValue = document.hasFocus() ? 'focused' : document.visibilityState;
  writeAtom(pageVisibility, oldValue => {
    if (oldValue !== newValue && newValue === 'hidden') {
      clearTrackedCache();
    }
    return newValue;
  });
};

window.addEventListener('focus', handleVisibilityChange);
window.addEventListener('blur', handleVisibilityChange);
document.addEventListener('visibilitychange', handleVisibilityChange);
registerCleanup(
  pageVisibility,
  () => {
    document.removeEventListener('visibilitychange', handleVisibilityChange);
    window.removeEventListener('focus', handleVisibilityChange);
    window.removeEventListener('blur', handleVisibilityChange);
  },
  import.meta.hot,
);
