/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DiffId, DiffSummary, PageVisibility, RepoInfo, Result} from '../types';
import type {UICodeReviewProvider} from './UICodeReviewProvider';

import serverAPI from '../ClientToServerAPI';
// @fb-only
import {GithubUICodeReviewProvider} from './github/github';
import {atom, selector, selectorFamily} from 'recoil';
import {debounce} from 'shared/debounce';

export const repositoryInfo = atom<RepoInfo | null>({
  key: 'repositoryInfo',
  default: null,
  effects: [
    ({setSelf}) => {
      const disposable = serverAPI.onMessageOfType('repoInfo', event => {
        setSelf(event.info);
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onConnectOrReconnect(() =>
        serverAPI.postMessage({
          type: 'requestRepoInfo',
        }),
      ),
  ],
});

export const codeReviewProvider = selector<UICodeReviewProvider | null>({
  key: 'codeReviewProvider',
  get: ({get}) => {
    const repoInfo = get(repositoryInfo);
    if (repoInfo?.type !== 'success') {
      return null;
    }
    if (repoInfo.codeReviewSystem.type === 'github') {
      return new GithubUICodeReviewProvider(repoInfo.preferredSubmitCommand ?? 'pr');
    }
    // prettier-ignore
    // @fb-only
    return null;
  },
});

export const diffSummary = selectorFamily<Result<DiffSummary | undefined>, DiffId>({
  key: 'diffSummary',
  get:
    diffId =>
    ({get}) => {
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
      serverAPI.onConnectOrReconnect(() =>
        serverAPI.postMessage({
          type: 'fetchDiffSummaries',
        }),
      ),
  ],
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
