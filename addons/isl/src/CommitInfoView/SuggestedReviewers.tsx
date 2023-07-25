/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';

import {T} from '../i18n';

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

export function SuggestedReviewers({
  existingReviewers,
  addReviewer,
}: {
  existingReviewers: Array<string>;
  addReviewer: (value: string) => unknown;
}) {
  const suggested = ['muirdm', 'quark', 'person1', 'person2', 'person3'].filter(
    s => !existingReviewers.includes(s),
  );
  const recent = recentReviewers.getRecent().filter(s => !existingReviewers.includes(s));
  return (
    <div className="suggested-reviewers">
      <div>
        <div className="suggestion-header">
          <T>Suggested</T>
        </div>
        <div className="suggestions">
          {suggested.map(s => (
            <Suggestion key={s} onClick={() => addReviewer(s)}>
              {s}
            </Suggestion>
          ))}
        </div>
      </div>
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
