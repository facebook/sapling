/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import serverAPI from '../ClientToServerAPI';
import {tracker} from '../analytics';
import {codeReviewProvider} from '../codeReview/CodeReviewInfo';
import {T} from '../i18n';
import {atomFamilyWeak} from '../jotaiUtils';
import {uncommittedChangesWithPreviews} from '../previews';
import {commitByHash} from '../serverAPIState';
import {commitInfoViewCurrentCommits, commitMode} from './CommitInfoState';
import {atom, useAtomValue} from 'jotai';
import {loadable} from 'jotai/utils';
import {Icon} from 'shared/Icon';
import {tryJsonParse} from 'shared/utils';

import './SuggestedReviewers.css';

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
      this.recent = new Map(
        (tryJsonParse(localStorage.getItem(RECENT_REVIEWERS_STORAGE_KEY) ?? '[]') as Array<
          [string, number]
        >) ?? [],
      );
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

/**
 * Since we use a selector to fetch suggestions, it will attempt to refetch
 * when any dependency (uncommitted changes, list of changed files) changes.
 * While technically suggestions could change if any edited path changes,
 * the UI flickers way to much. So let's cache the result within some time window.
 * using a time window ensures we don't overcache (for example,
 * in commit mode, where two commits may have totally different changes.)
 */
const cachedSuggestions = new Map<string, {lastFetch: number; reviewers: Array<string>}>();
const MAX_SUGGESTION_CACHE_AGE = 2 * 60 * 1000;
const suggestedReviewersForCommit = atomFamilyWeak((hashOrHead: string | 'head' | undefined) => {
  return loadable(
    atom(get => {
      if (hashOrHead == null) {
        return [];
      }
      const context = {
        paths: [] as Array<string>,
      };
      const cached = cachedSuggestions.get(hashOrHead);
      if (cached) {
        if (Date.now() - cached.lastFetch < MAX_SUGGESTION_CACHE_AGE) {
          return cached.reviewers;
        }
      }

      if (hashOrHead === 'head') {
        const uncommittedChanges = get(uncommittedChangesWithPreviews);
        context.paths.push(...uncommittedChanges.slice(0, 10).map(change => change.path));
      } else {
        const commit = get(commitByHash(hashOrHead));
        if (commit?.isDot) {
          const uncommittedChanges = get(uncommittedChangesWithPreviews);
          context.paths.push(...uncommittedChanges.slice(0, 10).map(change => change.path));
        }
        context.paths.push(...(commit?.filesSample.slice(0, 10).map(change => change.path) ?? []));
      }

      return tracker.operation('GetSuggestedReviewers', 'FetchError', undefined, async () => {
        serverAPI.postMessage({
          type: 'getSuggestedReviewers',
          key: hashOrHead,
          context,
        });

        const response = await serverAPI.nextMessageMatching(
          'gotSuggestedReviewers',
          message => message.key === hashOrHead,
        );
        cachedSuggestions.set(hashOrHead, {lastFetch: Date.now(), reviewers: response.reviewers});
        return response.reviewers;
      });
    }),
  );
});

export function SuggestedReviewers({
  existingReviewers,
  addReviewer,
}: {
  existingReviewers: Array<string>;
  addReviewer: (value: string) => unknown;
}) {
  const provider = useAtomValue(codeReviewProvider);
  const recent = recentReviewers.getRecent().filter(s => !existingReviewers.includes(s));
  const mode = useAtomValue(commitMode);
  const currentCommitInfoViewCommit = useAtomValue(commitInfoViewCurrentCommits);
  const currentCommit = currentCommitInfoViewCommit?.[0]; // assume we only have one commit

  const key = currentCommit?.isDot && mode === 'commit' ? 'head' : currentCommit?.hash ?? '';
  const suggestedReviewers = useAtomValue(suggestedReviewersForCommit(key));

  const filteredSuggestions = (
    suggestedReviewers.state === 'hasData' ? suggestedReviewers.data : []
  ).filter(s => !existingReviewers.includes(s));

  return (
    <div className="suggested-reviewers" data-testid="suggested-reviewers">
      {provider?.supportsSuggestedReviewers &&
      (filteredSuggestions == null || filteredSuggestions.length > 0) ? (
        <div data-testid="suggested-reviewers-list">
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
              <Suggestion
                key={s}
                onClick={() => {
                  addReviewer(s);
                  tracker.track('AcceptSuggestedReviewer', {extras: {type: 'suggested'}});
                }}>
                {s}
              </Suggestion>
            )) ?? null}
          </div>
        </div>
      ) : null}
      {recent.length > 0 ? (
        <div data-testid="recent-reviewers-list">
          <div className="suggestion-header">
            <T>Recent</T>
          </div>
          <div className="suggestions">
            {recent.map(s => (
              <Suggestion
                key={s}
                onClick={() => {
                  addReviewer(s);
                  tracker.track('AcceptSuggestedReviewer', {extras: {type: 'recent'}});
                }}>
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
