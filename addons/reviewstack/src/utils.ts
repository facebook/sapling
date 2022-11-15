/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitChange} from './github/diffTypes';
import type {GitHubPullRequestReviewThread} from './github/pullRequestTimelineTypes';
import type {TreeEntry} from './github/types';
import type {LabelColorOptions} from '@primer/react/lib/Label';

import {DiffSide, PullRequestReviewDecision} from './generated/graphql';
import joinPath from './joinPath';

export function formatISODate(iso: string, withTime = true): string {
  const date = new Date(iso);
  const options: Intl.DateTimeFormatOptions = {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
  };

  if (withTime) {
    Object.assign(options, {
      hour: 'numeric',
      minute: 'numeric',
    });
  }

  return date.toLocaleString(undefined, options);
}

export function shortOid(oid: string): string {
  return oid.slice(0, 8);
}

export function versionLabel(index: number): string {
  return `Version ${index + 1}`;
}

export function groupBy<TKey, TValue>(
  values: TValue[],
  getKey: (value: TValue) => TKey | null,
): Map<TKey, TValue[]> {
  return values.reduce((acc, value) => {
    const key = getKey(value);
    if (key != null) {
      let valuesForKey = acc.get(key);
      if (valuesForKey == null) {
        valuesForKey = [];
        acc.set(key, valuesForKey);
      }
      valuesForKey.push(value);
    }
    return acc;
  }, new Map());
}

export function groupByDiffSide<TValue>(
  values: TValue[],
  getKey: (value: TValue) => DiffSide | null,
): {[key in DiffSide]: TValue[]} {
  return values.reduce(
    (acc, value) => {
      const key = getKey(value);
      if (key != null) {
        const list: TValue[] = acc[key];
        list.push(value);
      }
      return acc;
    },
    {
      [DiffSide.Left]: [],
      [DiffSide.Right]: [],
    },
  );
}

export function countCommentsForThreads(threads: GitHubPullRequestReviewThread[]): number {
  return threads.reduce((acc, thread) => acc + (thread.comments.length ?? 0), 0);
}

/**
 * Split a path into [dirname, basename].
 */
export function splitPath(path: string): [string, string] {
  const index = path.lastIndexOf('/');
  if (index !== -1) {
    return [path.slice(0, index), path.slice(index + 1)];
  } else {
    return ['', path];
  }
}

export function getPathForChange(change: CommitChange): string {
  switch (change.type) {
    case 'add':
      return joinPath(change.basePath, change.entry.name);
    case 'remove':
      return joinPath(change.basePath, change.entry.name);
    case 'modify':
      return joinPath(change.basePath, change.before.name);
  }
}

export function getTreeEntriesForChange(change: CommitChange): {
  before: TreeEntry | null;
  after: TreeEntry | null;
} {
  switch (change.type) {
    case 'add': {
      return {
        before: null,
        after: change.entry,
      };
    }
    case 'remove': {
      return {
        before: change.entry,
        after: null,
      };
    }
    case 'modify': {
      return {
        before: change.before,
        after: change.after,
      };
    }
  }
}

export function pullRequestReviewDecisionLabel(reviewDecision: PullRequestReviewDecision): {
  label: string;
  variant: LabelColorOptions;
} {
  switch (reviewDecision) {
    case PullRequestReviewDecision.Approved:
      return {label: 'Approved', variant: 'success'};
    case PullRequestReviewDecision.ChangesRequested:
      return {label: 'Changes Requested', variant: 'danger'};
    case PullRequestReviewDecision.ReviewRequired:
      return {label: 'Review Required', variant: 'attention'};
  }
}
