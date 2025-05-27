/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RepoPath} from 'shared/types/common';
import type {FileFlag} from './common';

/***
 * Commit diff in a JSON-friendly format.
 * Lossly. Might not include all changes in the commit (ex. large or binary
 * files).
 * Initially intended as input to a remote "split" service.
 */
export type DiffCommit = {
  /*** Commit message. */
  message: string;
  /*** Diff-able (text and non-large) files. */
  files: ReadonlyArray<DiffFile>;
};

/**
 * Additional args.
 */
export type Args = {
  /** User specified prompt. */
  user_prompt?: string | null;
};

/** Unified diff represent in a JSON-friendly format. */
export type DiffFile = {
  /** File path on the left side (previous version). */
  aPath: RepoPath;
  /** File path on the right side (current version). */
  bPath: RepoPath;
  /**
   * File flag on the left side (previous version).
   * '': normal; 'x': executable; 'l': symlink; 'a': absent (deleted); 'm': submodule.
   * Cannot be ".".
   */
  aFlag: FileFlag;
  /** File flag on the right side (current version). */
  bFlag: FileFlag;
  /** Unified diff. See `DiffLine`. */
  lines: ReadonlyArray<DiffLine>;
};

/** A line in unified diff. */
export type DiffLine = {
  /**
   * Line number on the left side (previous version).
   * Starting from 0.
   * `null` means the line does not exist on the left side,
   * aka. the line was added.
   */
  a: number | null;
  /**
   * Line number on the right side (current version).
   * Starting from 0.
   * `null` means the line does not exist on the right side,
   * aka. the line was deleted.
   */
  b: number | null;
  /**
   * Line content.
   * Trailing new-line is preserved.
   * The last line might have no trailing new-line.
   */
  content: string;
};

/** Selects a subset of changes of a `DiffCommit` as a new commit. */
export type PartiallySelectedDiffCommit = {
  message: string;
  files: ReadonlyArray<PartiallySelectedDiffFile>;
};

/**
 * Selects a subset of `DiffFile` as part of a new commit.
 * The "a" side cannot be changed.
 * This only affects the "b" side.
 */
export type PartiallySelectedDiffFile = {
  bPath: RepoPath;
  /** File flag. Default: ''. */
  bFlag?: FileFlag;
  /** A subset of selected lines on the "a" side. */
  aLines: ReadonlyArray<number>;
  /** A subset of selected lines on the "b" side. */
  bLines: ReadonlyArray<number>;
};
