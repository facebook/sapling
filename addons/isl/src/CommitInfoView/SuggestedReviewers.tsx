/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import serverAPI from '../ClientToServerAPI';
import {codeReviewProvider} from '../codeReview/CodeReviewInfo';
import {T} from '../i18n';
import {uncommittedChangesWithPreviews} from '../previews';
import {commitByHash} from '../serverAPIState';
import {commitInfoViewCurrentCommits, commitMode} from './CommitInfoState';
import {selectorFamily, useRecoilValue, useRecoilValueLoadable} from 'recoil';
import {Icon} from 'shared/Icon';

import './SuggestedReviewers.css';

const tryParse = (s: string | null): Array<[string, number]> | undefined => {
  if (s == null) {
    return undefined;
  }
  try {
    return JSON.parse(s);
  } catch {
    return undefined;
  }
};

const MAX_VISIBLE_RECENT_REVIEWERS = 3;
const RECENT_REVIEWERS_STORAGE_KEY = 'ISL_RECENT_REVIEWERS';
/**
 * Simple counter for most used recent reviewers, persisted to localstorage if possible.
 * TODO: use "frecency": combined heuristic for least-recently-used plus most-often-used.
 */
class RecentReviewers {
  private recent: Map<string, number>;

  constructor() {
    try {
      this.recent = new Map(tryParse(localStorage.getItem(RECENT_REVIEWERS_STORAGE_KEY)) ?? []);
    } catch {
      this.recent = new Map();
    }
  }

  private persist() {
    try {
      localStorage.setItem(
        RECENT_REVIEWERS_STORAGE_KEY,
        JSON.stringify([...this.recent.entries()]),
      );
    } catch {}
  }

  public useReviewer(reviewer: string) {
    const existing = this.recent.get(reviewer) ?? 0;
    this.recent.set(reviewer, existing + 1);
    this.persist();
  }

  public getRecent(): Array<string> {
    return [...this.recent.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, MAX_VISIBLE_RECENT_REVIEWERS)
      .map(([k]) => k);
  }
}

export const recentReviewers = new RecentReviewers();

const suggestedReviewersForCommit = selectorFamily<Array<string>, string>({
  key: 'suggestedReviewersForCommit',
  get:
    (hashOrHead: string | 'head' | undefined) =>
    async ({get}) => {
      if (hashOrHead == null) {
        return [];
      }
      const context = {
        paths: [] as Array<string>,
      };
      if (hashOrHead === 'head') {
        const uncommittedChanges = get(uncommittedChangesWithPreviews);
        context.paths.push(...uncommittedChanges.slice(0, 10).map(change => change.path));
      } else {
        const commit = get(commitByHash(hashOrHead));
        if (commit?.isHead) {
          const uncommittedChanges = get(uncommittedChangesWithPreviews);
          context.paths.push(...uncommittedChanges.slice(0, 10).map(change => change.path));
        }
        context.paths.push(...(commit?.filesSample.slice(0, 10).map(change => change.path) ?? []));
      }

      serverAPI.postMessage({
        type: 'getSuggestedReviewers',
        key: hashOrHead,
        context,
      });

      const response = await serverAPI.nextMessageMatching(
        'gotSuggestedReviewers',
        message => message.key === hashOrHead,
      );
      return response.reviewers;
    },
});

export function SuggestedReviewers({
  existingReviewers,
  addReviewer,
}: {
  existingReviewers: Array<string>;
  addReviewer: (value: string) => unknown;
}) {
  const provider = useRecoilValue(codeReviewProvider);
  const recent = recentReviewers.getRecent().filter(s => !existingReviewers.includes(s));
  const mode = useRecoilValue(commitMode);
  const currentCommitInfoViewCommit = useRecoilValue(commitInfoViewCurrentCommits);
  const currentCommit = currentCommitInfoViewCommit?.[0]; // assume we only have one commit

  const key = currentCommit?.isHead && mode === 'commit' ? 'head' : currentCommit?.hash ?? '';
  const suggestedReviewers = useRecoilValueLoadable(suggestedReviewersForCommit(key));

  const filteredSuggestions = suggestedReviewers
    .valueMaybe()
    ?.filter(s => !existingReviewers.includes(s))
    .filter(() => false);

  return (
    <div className="suggested-reviewers">
      {provider?.supportsSuggestedReviewers &&
      (filteredSuggestions == null || filteredSuggestions.length) > 0 ? (
        <div>
          <div className="suggestion-header">
            <T>Suggested</T>
          </div>
          <div className="suggestions">
            {suggestedReviewers.state === 'loading' && (
              <div className="suggestions-loading">
                <Icon icon="loading" />
              </div>
            )}
            {filteredSuggestions?.map(s => (
              <Suggestion key={s} onClick={() => addReviewer(s)}>
                {s}
              </Suggestion>
            )) ?? null}
          </div>
        </div>
      ) : null}
      {recent.length > 0 ? (
        <div>
          <div className="suggestion-header">
            <T>Recent</T>
          </div>
          <div className="suggestions">
            {recent.map(s => (
              <Suggestion key={s} onClick={() => addReviewer(s)}>
                {s}
              </Suggestion>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function Suggestion({children, onClick}: {children: ReactNode; onClick: () => unknown}) {
  return (
    <button className="suggestion token" onClick={onClick}>
      {children}
    </button>
  );
}
