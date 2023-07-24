/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, DiffSummary, PageVisibility, Result} from '../types';
import type {UICodeReviewProvider} from './UICodeReviewProvider';

import serverAPI from '../ClientToServerAPI';
import {Internal} from '../Internal';
import {treeWithPreviews} from '../previews';
import {commitByHash, repositoryInfo} from '../serverAPIState';
import {GithubUICodeReviewProvider} from './github/github';
import {atom, selector, selectorFamily} from 'recoil';
import {debounce} from 'shared/debounce';

export const codeReviewProvider = selector<UICodeReviewProvider | null>({
  key: 'codeReviewProvider',
  get: ({get}) => {
    const repoInfo = get(repositoryInfo);
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
  },
});

export const diffSummary = selectorFamily<Result<DiffSummary | undefined>, DiffId | undefined>({
  key: 'diffSummary',
  get:
    diffId =>
    ({get}) => {
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
    },
});

export const allDiffSummaries = atom<Result<Map<DiffId, DiffSummary> | null>>({
  key: 'allDiffSummaries',
  default: {value: null},
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('fetchedDiffSummaries', event => {
        setSelf(event.summaries);
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onSetup(() =>
        serverAPI.postMessage({
          type: 'fetchDiffSummaries',
        }),
      ),
  ],
});

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
export const latestCommitMessage = selectorFamily<[title: string, description: string], string>({
  key: 'latestCommitMessage',
  get:
    (hash: string) =>
    ({get}) => {
      const commit = get(commitByHash(hash));
      const preview = get(treeWithPreviews).treeMap.get(hash)?.info;

      if (
        preview != null &&
        (preview.title !== commit?.title || preview.description !== commit?.description)
      ) {
        return [preview.title, preview.description];
      }

      if (!commit) {
        return ['', ''];
      }

      const localTitle = commit.title;
      const localDescription = commit.description;

      const remoteTitle = commit.diffId
        ? // TODO: try to get the commit title from the latest data for this commit's diff fetched from the remote
          localTitle
        : localTitle;
      const remoteDescription = commit.diffId
        ? // TODO: try to get the commit message from the latest data for this commit's diff fetched from the remote
          localDescription
        : localDescription;

      return [remoteTitle ?? localTitle, remoteDescription ?? localDescription];
    },
});

export const pageVisibility = atom<PageVisibility>({
  key: 'pageVisibility',
  default: document.hasFocus() ? 'focused' : document.visibilityState,
  effects: [
    ({setSelf}) => {
      const handleVisibilityChange = () => {
        setSelf(document.hasFocus() ? 'focused' : document.visibilityState);
      };

      window.addEventListener('focus', handleVisibilityChange);
      window.addEventListener('blur', handleVisibilityChange);
      document.addEventListener('visibilitychange', handleVisibilityChange);
      return () => {
        document.removeEventListener('visibilitychange', handleVisibilityChange);
        window.removeEventListener('focus', handleVisibilityChange);
        window.removeEventListener('blur', handleVisibilityChange);
      };
    },
    ({onSet}) => {
      onSet(
        debounce(state => {
          serverAPI.postMessage({
            type: 'pageVisibility',
            state,
          });
        }, 50),
      );
    },
  ],
});
