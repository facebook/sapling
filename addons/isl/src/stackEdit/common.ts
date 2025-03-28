/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RecordOf} from 'immutable';
import type {Author, Hash, RepoPath} from 'shared/types/common';
import type {FileFlag} from 'shared/types/stack';

import {Map as ImMap, Set as ImSet, List, Record, is} from 'immutable';

/** Check if a path at the given commit is a rename. */
export function isRename(commit: CommitState, path: RepoPath): boolean {
  const files = commit.files;
  const copyFromPath = files.get(path)?.copyFrom;
  if (copyFromPath == null) {
    return false;
  }
  return isAbsent(files.get(copyFromPath));
}

/** Test if a file is absent. */
export function isAbsent(file: FileState | FileMetadata | undefined): boolean {
  if (file == null) {
    return true;
  }
  return file.flags === ABSENT_FLAG;
}

/** Test if a file has utf-8 content. */
export function isUtf8(file: FileState): boolean {
  return typeof file.data === 'string' || file.data instanceof FileIdx;
}

/** Test if 2 files have the same content, ignoring "copyFrom". */
export function isContentSame(file1: FileState, file2: FileState): boolean {
  return is(file1.data, file2.data) && (file1.flags ?? '') === (file2.flags ?? '');
}

/** Extract metadata */
export function toMetadata(file: FileState): FileMetadata {
  return FileMetadata({copyFrom: file.copyFrom, flags: file.flags});
}

type DateTupleProps = {
  /** UTC Unix timestamp in seconds. */
  unix: number;
  /** Timezone offset in minutes. */
  tz: number;
};

export const DateTuple = Record<DateTupleProps>({unix: 0, tz: 0});
export type DateTuple = RecordOf<DateTupleProps>;

/** Mutable commit state. */
export type CommitStateProps = {
  rev: CommitRev;
  /** Original hashes. Used for "predecessor" information. */
  originalNodes: ImSet<Hash>;
  /**
   * Unique identifier within the stack. Useful for React animation.
   *
   * Note this should not be a random string, since we expect the CommitState[]
   * state to be purely derived from the initial ExportStack. It makes it easier
   * to check what commits are actually modified by just comparing CommitStates.
   * The "skip unchanged commits" logic is used by `calculateImportStack()`.
   *
   * We use commit hashes initially. When there is a split or add a new commit,
   * we assign new keys in a predicable (non-random) way. This property is
   * never empty, unlike `originalNodes`.
   */
  key: string;
  author: Author;
  date: DateTuple;
  /** Commit message. */
  text: string;
  /**
   * - hash: commit hash is immutable; this commit and ancestors
   *   cannot be edited in any way.
   * - content: file contents are immutable; commit hash can change
   *   if ancestors are changed.
   * - diff: file changes (diff) are immutable; file contents or
   *   commit hash can change if ancestors are changed.
   * - none: nothing is immutable; this commit can be edited.
   */
  immutableKind: 'hash' | 'content' | 'diff' | 'none';
  /** Parent commits. */
  parents: List<CommitRev>;
  /** Changed files. */
  files: ImMap<RepoPath, FileState>;
};

export const CommitState = Record<CommitStateProps>({
  rev: 0 as CommitRev,
  originalNodes: ImSet(),
  key: '',
  author: '',
  date: DateTuple(),
  text: '',
  immutableKind: 'none',
  parents: List(),
  files: ImMap(),
});
export type CommitState = RecordOf<CommitStateProps>;

/**
 * Similar to `ExportFile` but `data` can be lazy by redirecting to a rev in a file stack.
 * Besides, supports "absent" state.
 */
type FileStateProps = {
  data: string | Base85 | FileIdx | DataRef;
} & FileMetadataProps;

/**
 * File metadata properties without file content.
 */
type FileMetadataProps = {
  /** If present, this file is copied (or renamed) from another file. */
  copyFrom?: RepoPath;
  /**
   * If present, whether this file is special (symlink, submodule, deleted,
   * executable).
   */
  flags?: FileFlag;
};

type Base85Props = {dataBase85: string};
export const Base85 = Record<Base85Props>({dataBase85: ''});
export type Base85 = RecordOf<Base85Props>;

type DataRefProps = {node: Hash; path: RepoPath};
export const DataRef = Record<DataRefProps>({node: '', path: ''});
export type DataRef = RecordOf<DataRefProps>;

export const FileState = Record<FileStateProps>({data: '', copyFrom: undefined, flags: ''});
export type FileState = RecordOf<FileStateProps>;

export const FileMetadata = Record<FileMetadataProps>({copyFrom: undefined, flags: ''});
export type FileMetadata = RecordOf<FileMetadataProps>;

export type FileStackIndex = number;

type FileIdxProps = {
  fileIdx: FileStackIndex;
  fileRev: FileRev;
};

type CommitIdxProps = {
  rev: CommitRev;
  path: RepoPath;
};

export const FileIdx = Record<FileIdxProps>({fileIdx: 0, fileRev: 0 as FileRev});
export type FileIdx = RecordOf<FileIdxProps>;

export const CommitIdx = Record<CommitIdxProps>({rev: -1 as CommitRev, path: ''});
export type CommitIdx = RecordOf<CommitIdxProps>;

export const ABSENT_FLAG = 'a';

/**
 * Represents an absent (or deleted) file.
 *
 * Helps simplify `null` handling logic. Since `data` is a regular
 * string, an absent file can be compared (data-wise) with its
 * adjacent versions and edited. This makes it easier to, for example,
 * split a newly added file.
 */
export const ABSENT_FILE = FileState({
  data: '',
  flags: ABSENT_FLAG,
});

/** A revision number used in the `FileStackState`. Identifies a version of a multi-version file. */
export type FileRev = number & {__brand: 'FileStackRev'};

/** A revision number used in the `CommitStackState`. Identifies a commit in the stack. */
export type CommitRev = number & {__branded: 'CommitRev'};

// Re-export
export type {FileFlag};
