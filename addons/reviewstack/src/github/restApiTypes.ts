/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DateTime, GitObjectID} from './types';

type CommitParent = {
  sha: GitObjectID;
};

export type Commit = {
  sha: GitObjectID;
  author: {
    login: string;
  } | null;
  commit: {
    author: {
      name: string;
    };
    committer: {
      date: DateTime;
    };
    message: string;
    tree: {
      sha: GitObjectID;
    };
  };
  parents: CommitParent[];
};

/** See https://docs.github.com/en/rest/reference/commits#compare-two-commits */
export type CommitComparison = {
  mergeBaseCommit: {
    sha: GitObjectID;
    commit: {
      committer: {
        date: DateTime;
      };
    };
  };
  commits: Commit[];
};
