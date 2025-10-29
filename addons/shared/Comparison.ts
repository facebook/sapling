/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export enum ComparisonType {
  UncommittedChanges = 'Uncommitted',
  HeadChanges = 'Head',
  StackChanges = 'Stack',
  Committed = 'Commit',
  SinceLastCodeReviewSubmit = 'SinceLastCodeReviewSubmit',
}

export type Comparison =
  | {
      type: ComparisonType.Committed;
      hash: string;
    }
  | {
      /**
       * Compare this hash against the version last submit for code review.
       * Only valid if supported by the UICodeReviewProvider
       */
      type: ComparisonType.SinceLastCodeReviewSubmit;
      hash: string;
    }
  | {
      type:
        | ComparisonType.UncommittedChanges
        | ComparisonType.HeadChanges
        | ComparisonType.StackChanges;
    };

export function isComparison(arg: unknown): arg is Comparison {
  return (
    arg != null &&
    typeof arg === 'object' &&
    Object.values(ComparisonType).includes((arg as Comparison).type)
  );
}

export function comparisonIsAgainstHead(comparison: Comparison): boolean {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
    case ComparisonType.HeadChanges:
    case ComparisonType.StackChanges:
      return true;
    case ComparisonType.SinceLastCodeReviewSubmit:
      return false;
    case ComparisonType.Committed:
      return false;
  }
}

/** Give a stable string key to uniquely represent a Comparison */
export function comparisonStringKey(comparison: Comparison): string {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
    case ComparisonType.HeadChanges:
    case ComparisonType.StackChanges:
      return comparison.type;
    case ComparisonType.SinceLastCodeReviewSubmit:
    case ComparisonType.Committed:
      return `${comparison.type}:${comparison.hash}`;
  }
}

/** Arguments for a comparison, compatible with `sl diff`. */
export function revsetArgsForComparison(comparison: Comparison): Array<string> {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
      return ['--rev', '.'];
    case ComparisonType.HeadChanges:
      return ['--rev', '.^'];
    case ComparisonType.StackChanges:
      return ['--rev', 'ancestor(.,interestingmaster())'];
    case ComparisonType.SinceLastCodeReviewSubmit:
      return ['--rev', comparison.hash, '--since-last-submit'];
    case ComparisonType.Committed:
      return ['--change', comparison.hash];
  }
}

/** Revset for a comparison, compatible with `sl cat`. */
export function revsetForComparison(comparison: Comparison): string {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
      return '.';
    case ComparisonType.HeadChanges:
      return '.^';
    case ComparisonType.StackChanges:
      return 'ancestor(.,interestingmaster())';
    case ComparisonType.Committed:
      return comparison.hash;
    case ComparisonType.SinceLastCodeReviewSubmit:
      return comparison.hash;
  }
}

/** Revset for an `sl cat` comparison, to get the content of the "before" (left) side of this comparison. */
export function beforeRevsetForComparison(comparison: Comparison): string {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
      return '.'; // this is in the head commit, not in the wdir
    case ComparisonType.HeadChanges:
      return '.^'; // in the parent commit
    case ComparisonType.StackChanges:
      return 'ancestor(.,interestingmaster())'; // in the public base itself
    case ComparisonType.Committed:
      return comparison.hash + '^'; // before this commit
    case ComparisonType.SinceLastCodeReviewSubmit:
      return comparison.hash + '^';
  }
}

/** Revset for an `sl cat` comparison, to get the content of the "current" (right) side of this comparison. */
export function currRevsetForComparison(comparison: Comparison): string {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
      return 'wdir()';
    case ComparisonType.HeadChanges:
      return 'wdir()';
    case ComparisonType.StackChanges:
      return 'wdir()';
    case ComparisonType.Committed:
      return comparison.hash;
    case ComparisonType.SinceLastCodeReviewSubmit:
      return comparison.hash;
  }
}

/**
 * English description of comparison.
 * Note: non-localized. Don't forget to run this through `t()` for a given client.
 */
export function labelForComparison(comparison: Comparison): string {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
      return 'Uncommitted Changes';
    case ComparisonType.HeadChanges:
      return 'Head Changes';
    case ComparisonType.StackChanges:
      return 'Stack Changes';
    case ComparisonType.Committed:
      return `In ${comparison.hash.slice(0, 12)}`;
    case ComparisonType.SinceLastCodeReviewSubmit:
      return `Since last submit of ${comparison.hash.slice(0, 12)}`;
  }
}
