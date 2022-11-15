/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export enum ComparisonType {
  UncommittedChanges = 'UNCOMMITTED',
  HeadChanges = 'HEAD',
  StackChanges = 'STACK',
  Committed = 'InCommit',
}

export type Comparison =
  | {
      type: ComparisonType.Committed;
      hash: string;
    }
  | {
      type:
        | ComparisonType.UncommittedChanges
        | ComparisonType.HeadChanges
        | ComparisonType.StackChanges;
    };

/** Arguments for a comparison, compatible with `sl diff`. */
export function revsetArgsForComparison(comparison: Comparison): Array<string> {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
      return ['--rev', '.'];
    case ComparisonType.HeadChanges:
      return ['--rev', '.^'];
    case ComparisonType.StackChanges:
      return ['--rev', 'ancestor(.,interestingmaster())'];
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
      return `In ${comparison.hash}`;
  }
}
