/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Rev} from './fileStackState';
import type {Author, DateTuple, Hash, RepoPath} from 'shared/types/common';
import type {ExportStack, ExportFile, Mark} from 'shared/types/stack';

import {assert} from '../utils';
import {FileStackState} from './fileStackState';
import {unwrap} from 'shared/utils';

/**
 * A stack of commits with stack editing features.
 *
 * Provides read write APIs for editing the stack.
 * Under the hood, continuous changes to a same file are grouped
 * to file stacks. Part of analysis and edit operations are deletegated
 * to corrosponding file stacks.
 */
export class CommitStackState {
  /**
   * Original stack exported by `debugexportstack`. Immutable.
   * Useful to calculate "predecessor" information.
   */
  originalStack: ExportStack;

  /**
   * File contents at the bottom of the stack.
   *
   * For example, when editing stack with two commits A and B:
   *
   * ```
   *    B <- draft, rev 2
   *    |
   *    A <- draft, modifies foo.txt, rev 1
   *   /
   *  P <- public, does not modify foo.txt, rev 0
   * ```
   *
   * `bottomFiles['foo.txt']` would be the `foo.txt` content in P,
   * despite P does not change `foo.txt`.
   *
   * `bottomFiles` are considered immutable - stack editing operations
   * won't change `bottomFiles` directly.
   *
   * This also assumes there are only one root of the stack.
   *
   * This implies that: every file referenced or edited by any commit
   * in the stack will be present in this map. If a file was added
   * later in the stack, it is in this map and marked as absent.
   */
  bottomFiles: Map<RepoPath, ExportFile>;

  /**
   * Mutable commit stack. Indexed by rev.
   * Only stores "modified (added, edited, deleted)" files.
   */
  stack: CommitState[];

  /**
   * File stack states.
   * They are constructed on demand, and provide advanced features.
   */
  fileStacks: FileStackState[] = [];

  /**
   * Map from `${commitRev}:${path}` to FileStack index and rev.
   * Note the commitRev could be -1, meaning that `bottomFiles` is used.
   */
  commitToFile: Map<string, [FileStackIndex, Rev]> = new Map();

  /**
   * Map from `${fileStackIndex}:${fileRev}` to commitRev and path.
   * Note the commitRev could be -1, meaning that `bottomFiles` is used.
   */
  fileToCommit: Map<string, [Rev, RepoPath]> = new Map();

  // Initial setup.

  /** Construct from an exported stack. */
  constructor(stack: ExportStack) {
    this.originalStack = stack;
    this.bottomFiles = getBottomFilesFromExportStack(stack);
    this.stack = getCommitStatesFromExportStack(stack);
    this.buildFileStacks();
  }

  // Read operations.

  /** Returns all valid revs. */
  revs(): Rev[] {
    return [...this.stack.keys()];
  }

  /**
   * Get the file at the given `rev`.
   *
   * Returns `ABSENT_FILE` if the file does not exist in the commit.
   * Throws if the stack does not have information about the path.
   *
   * Note this is different from `this.stack[rev].files.get(path)`,
   * since `files` only tracks modified files, not existing files
   * created from the bottom of the stack.
   */
  getFile(rev: Rev, path: RepoPath): ExportFile {
    for (const logRev of this.log(rev)) {
      const commit = this.stack[logRev];
      const file = commit.files.get(path);
      if (file !== undefined) {
        // Commit modifieds `file`.
        return file;
      }
    }
    const file = this.bottomFiles.get(path) ?? ABSENT_FILE;
    if (file === undefined) {
      throw new Error(
        `file ${path} is not tracked by stack (tracked files: ${JSON.stringify(
          this.getAllPaths(),
        )})`,
      );
    }
    return file;
  }

  /** Get all file paths ever referred (via "copy from") or changed in the stack. */
  getAllPaths(): RepoPath[] {
    return [...this.bottomFiles.keys()].sort();
  }

  /** List revs, starting from the given rev. */
  *log(startRev: Rev): Generator<Rev, void> {
    const toVisit = [startRev];
    while (true) {
      const rev = toVisit.pop();
      if (rev === undefined) {
        break;
      }
      yield rev;
      const commit = this.stack[rev];
      // Visit parent commits.
      commit.parents.forEach(rev => {
        toVisit.push(rev);
      });
    }
  }

  /**
   * List revs that change the given file, starting from the given rev.
   * Optionally follow renames.
   */
  *logFile(
    startRev: Rev,
    startPath: RepoPath,
    followRenames = false,
  ): Generator<[Rev, RepoPath], void> {
    let path = startPath;
    for (const rev of this.log(startRev)) {
      const commit = this.stack[rev];
      const file = commit.files.get(path);
      if (file !== undefined) {
        yield [rev, path];
      }
      if (followRenames && file?.copyFrom) {
        path = file.copyFrom;
      }
    }
  }

  // File stack related.

  /**
   * (Re-)build file stacks and mappings.
   */
  buildFileStacks() {
    const fileStacks: FileStackState[] = [];
    const commitToFile = new Map<string, [FileStackIndex, Rev]>();
    const fileToCommit = new Map<string, [Rev, RepoPath]>();

    const processFile = (rev: Rev, file: ExportFile, path: RepoPath) => {
      const logFile = this.logFile(rev, path, true);
      let prevRev = -1;
      let prevPath = path;
      let prevFile = null;
      for (const [logRev, logPath] of logFile) {
        if (logRev !== rev) {
          [prevRev, prevPath] = [logRev, logPath];
          break;
        }
      }
      if (file.data != null) {
        // File was added or modified and has utf-8 content.
        let fileAppended = false;
        if (prevRev >= 0) {
          // Try to reuse an existing file stack.
          const prev = commitToFile.get(`${prevRev}:${prevPath}`);
          if (prev) {
            const [prevIdx, prevFileRev] = prev;
            const prevFileStack = fileStacks[prevIdx];
            // File stack history is linear. Only reuse it if its last
            // rev matches `prevFileRev`
            if (prevFileStack.revLength === prevFileRev + 1) {
              const fileRev = prevFileRev + 1;
              prevFileStack.editText(fileRev, file.data, false);
              commitToFile.set(`${rev}:${path}`, [prevIdx, fileRev]);
              fileToCommit.set(`${prevIdx}:${fileRev}`, [rev, path]);
              fileAppended = true;
            } else {
              prevFile = this.stack[prevRev].files.get(prevPath);
            }
          }
        }
        if (!fileAppended) {
          // Cannot reuse an existing file stack. Create a new file stack.
          const fileIdx = fileStacks.length;
          let fileTextList = [file.data];
          let fileRev = 0;
          prevFile ??= this.bottomFiles.get(path);
          if (prevFile?.data != null) {
            // Use "prevFile" as rev 0 (immutable public).
            fileTextList = [prevFile.data, file.data];
            commitToFile.set(`${prevRev}:${prevPath}`, [fileIdx, fileRev]);
            fileToCommit.set(`${fileIdx}:${fileRev}`, [prevRev, prevPath]);
            fileRev = 1;
          }
          const fileStack = new FileStackState(fileTextList);
          fileStacks.push(fileStack);
          commitToFile.set(`${rev}:${path}`, [fileIdx, fileRev]);
          fileToCommit.set(`${fileIdx}:${fileRev}`, [rev, path]);
        }
      }
    };

    this.stack.forEach((commit, rev) => {
      const files = commit.files;
      // Process order: renames, non-copy, copies.
      const priorityFiles: [number, RepoPath, ExportFile][] = [...files.entries()].map(
        ([path, file]) => {
          const priority = isRename(commit, path) ? 0 : file.copyFrom == null ? 1 : 2;
          return [priority, path, file];
        },
      );
      const renamed = new Set<RepoPath>();
      priorityFiles.sort().forEach(([priority, path, file]) => {
        // Skip already "renamed" absent files.
        let skip = false;
        if (priority === 0 && file.copyFrom != null) {
          renamed.add(file.copyFrom);
        } else {
          skip = isAbsent(file) && renamed.has(path);
        }
        if (!skip) {
          processFile(rev, file, path);
        }
      });
    });
    this.fileStacks = fileStacks;
    this.commitToFile = commitToFile;
    this.fileToCommit = fileToCommit;
  }

  /**
   * Describe all file stacks for testing purpose.
   * Each returned string represents a file stack.
   *
   * Output in `rev:commit/path(content)` format.
   * If `(content)` is left out it means the file at the rev is absent.
   * If `commit` is `.` then it comes from `bottomFiles` meaning that
   * the commit last modifies the path might be outside the stack.
   *
   * Rev 0 is usually the "public" version that is not editable.
   *
   * For example, `0:./x.txt 1:A/x.txt(33) 2:B/y.txt(33)` means:
   * commit A added `x.txt` with the content `33`, and commit B renamed it to
   * `y.txt`.
   *
   * `0:./z.txt(11) 1:A/z.txt(22) 2:C/z.txt` means: `z.txt` existed at
   * the bottom of the stack with the content `11`. Commit A modified
   * its content to `22` and commit C deleted `z.txt`.
   */
  describeFileStacks(showContent = true): string[] {
    const fileToCommit = this.fileToCommit;
    const stack = this.stack;
    return this.fileStacks.map((fileStack, fileIdx) => {
      return fileStack
        .revs()
        .map(fileRev => {
          const key = `${fileIdx}:${fileRev}`;
          const value = fileToCommit.get(key);
          const spans = [`${fileRev}:`];
          assert(value != null, 'fileToCommit should have all file stack revs');
          const [rev, path] = value;
          const [commitTitle, absent] =
            rev < 0
              ? ['.', isAbsent(this.bottomFiles.get(path))]
              : [
                  stack[rev].text.split('\n').at(0) || [...stack[rev].originalNodes].at(0) || '?',
                  isAbsent(stack[rev].files.get(path)),
                ];
          spans.push(`${commitTitle}/${path}`);
          if (showContent && !absent) {
            spans.push(`(${fileStack.get(fileRev)})`);
          }
          return spans.join('');
        })
        .join(' ');
    });
  }
}

function getBottomFilesFromExportStack(stack: ExportStack): Map<RepoPath, ExportFile> {
  // bottomFiles requires that the stack only has one root.
  checkStackSingleRoot(stack);

  // Calculate bottomFiles.
  const bottomFiles: Map<RepoPath, ExportFile> = new Map();
  stack.forEach(commit => {
    for (const [path, content] of Object.entries(commit.relevantFiles ?? {})) {
      if (!bottomFiles.has(path)) {
        bottomFiles.set(path, content ?? ABSENT_FILE);
      }
    }

    // Files not yet existed in `bottomFiles` means they are added (in root commits)
    // mark them as "missing" in the stack bottom.
    for (const path of Object.keys(commit.files ?? {})) {
      if (!bottomFiles.has(path)) {
        bottomFiles.set(path, ABSENT_FILE);
      }
    }
  });

  return bottomFiles;
}

function getCommitStatesFromExportStack(stack: ExportStack): CommitState[] {
  checkStackParents(stack);

  // Prepare nodeToRev convertion.
  const revs: Rev[] = [...stack.keys()];
  const nodeToRevMap: Map<Hash, Rev> = new Map(revs.map(rev => [stack[rev].node, rev]));
  const nodeToRev = (node: Hash): Rev => {
    const rev = nodeToRevMap.get(node);
    if (rev == null) {
      throw new Error(
        `Rev ${rev} should be known ${JSON.stringify(nodeToRevMap)} (bug in debugexportstack?)`,
      );
    }
    return rev;
  };

  // Calculate requested stack.
  return stack.map(commit => ({
    originalNodes: new Set([commit.node]),
    rev: nodeToRev(commit.node),
    author: commit.author,
    date: commit.date,
    text: commit.text,
    // Treat commits that are not requested explicitly as immutable too.
    immutableKind: commit.immutable || !commit.requested ? 'hash' : 'none',
    parents: (commit.parents ?? []).map(p => nodeToRev(p)),
    files: new Map(
      Object.entries(commit.files ?? {}).map(([path, file]) => [path, file ?? ABSENT_FILE]),
    ),
  }));
}

/** Check that there is only one root in the stack. */
function checkStackSingleRoot(stack: ExportStack) {
  const rootNodes = stack.filter(commit => (commit.parents ?? []).length === 0);
  if (rootNodes.length > 1) {
    throw new Error(
      `Multiple roots ${JSON.stringify(rootNodes.map(c => c.node))} is not supported`,
    );
  }
}

/**
 * Check the exported stack and throws if it breaks assumptions.
 * - No duplicated commits.
 * - "parents" refer to other commits in the stack.
 */
function checkStackParents(stack: ExportStack) {
  const knownNodes = new Set();
  stack.forEach(commit => {
    const parents = commit.parents ?? [];
    if (parents.length > 0) {
      if (!commit.requested) {
        throw new Error(
          `Requested commit ${commit.node} should not have parents ${JSON.stringify(
            parents,
          )} (bug in debugexportstack?)`,
        );
      }
      parents.forEach(parentNode => {
        if (!knownNodes.has(parentNode)) {
          throw new Error(`Parent commit ${parentNode} is not exported (bug in debugexportstack?)`);
        }
      });
    }
    if (parents.length > 1) {
      throw new Error(`Merge commit ${commit.node} is not supported`);
    }
    knownNodes.add(commit.node);
  });
  if (knownNodes.size != stack.length) {
    throw new Error('Commit stack has duplicated nodes (bug in debugexportstack?)');
  }
}

/** Check if a path at the given commit is a rename. */
function isRename(commit: CommitState, path: RepoPath): boolean {
  const files = commit.files;
  const copyFromPath = files.get(path)?.copyFrom;
  if (copyFromPath == null) {
    return false;
  }
  return isAbsent(files.get(copyFromPath));
}

/** Test if a file is absent. */
function isAbsent(file: ExportFile | undefined): boolean {
  if (file == null) {
    return true;
  }
  return file.flags === ABSENT_FLAG;
}

const ABSENT_FLAG = 'a';

/**
 * Represents an absent (or deleted) file.
 *
 * Helps simplify `null` handling logic. Since `data` is a regular
 * string, an absent file can be compared (data-wise) with its
 * adjacent versions and edited. This makes it easier to, for example,
 * split a newly added file.
 */
export const ABSENT_FILE: ExportFile = {
  data: '',
  flags: ABSENT_FLAG,
};

/** Mutable commit state. */
type CommitState = {
  rev: Rev;
  /** Original hashes. Used for "predecessor" information. */
  originalNodes: Set<Hash>;
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
  parents: Rev[];
  /** Changed files. */
  files: Map<RepoPath, ExportFile>;
};

type FileStackIndex = number;
