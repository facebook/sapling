/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Rev} from './fileStackState';
import type {Author, DateTuple, Hash, RepoPath} from 'shared/types/common';
import type {ExportStack, ExportFile} from 'shared/types/stack';

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
  bottomFiles: Map<RepoPath, FileState>;

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
  getFile(rev: Rev, path: RepoPath): FileState {
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
   * Get the parent version of a file and its introducing rev.
   * If the returned `rev` is -1, it means the file comes from
   * "bottomFiles", aka. its introducing rev is outside the stack.
   */
  parentFile(rev: Rev, path: RepoPath, followRenames = true): [Rev, RepoPath, FileState] {
    let prevRev = -1;
    let prevPath = path;
    let prevFile = unwrap(this.bottomFiles.get(path));
    const logFile = this.logFile(rev, path, followRenames);
    for (const [logRev, logPath] of logFile) {
      if (logRev !== rev) {
        [prevRev, prevPath] = [logRev, logPath];
        prevFile = unwrap(this.stack[prevRev].files.get(prevPath));
        break;
      }
    }
    return [prevRev, prevPath, prevFile];
  }

  /**
   * (Re-)build file stacks and mappings.
   */
  buildFileStacks() {
    const fileStacks: FileStackState[] = [];
    const commitToFile = new Map<string, [FileStackIndex, Rev]>();
    const fileToCommit = new Map<string, [Rev, RepoPath]>();

    const processFile = (rev: Rev, file: FileState, path: RepoPath) => {
      const [prevRev, prevPath, prevFile] = this.parentFile(rev, path);
      if (isUtf8(file)) {
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
              prevFileStack.editText(fileRev, this.getUtf8Data(file), false);
              commitToFile.set(`${rev}:${path}`, [prevIdx, fileRev]);
              fileToCommit.set(`${prevIdx}:${fileRev}`, [rev, path]);
              fileAppended = true;
            }
          }
        }
        if (!fileAppended) {
          // Cannot reuse an existing file stack. Create a new file stack.
          const fileIdx = fileStacks.length;
          let fileTextList = [this.getUtf8Data(file)];
          let fileRev = 0;
          if (isUtf8(prevFile)) {
            // Use "prevFile" as rev 0 (immutable public).
            fileTextList = [this.getUtf8Data(prevFile), ...fileTextList];
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

    // Migrate off 'fileStack' type, since we are going to replace the file stacks.
    this.useFileContent();

    this.stack.forEach((commit, rev) => {
      const files = commit.files;
      // Process order: renames, non-copy, copies.
      const priorityFiles: [number, RepoPath, FileState][] = [...files.entries()].map(
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
   * Switch file contents to use FileStack as source of truth.
   * Useful when using FileStack to edit files.
   */
  useFileStack() {
    this.forEachFile((rev, file, path) => {
      if (typeof file.data === 'string') {
        const fileIdxRev = this.commitToFile.get(`${rev}:${path}`);
        if (fileIdxRev != null) {
          const [fileIdx, fileRev] = fileIdxRev;
          file.data = {type: 'fileStack', rev: fileRev, index: fileIdx};
        }
      }
    });
  }

  /**
   * Switch file contents to use string as source of truth.
   * Useful when rebuilding FileStack.
   */
  useFileContent() {
    this.forEachFile((_rev, file) => {
      if (typeof file.data !== 'string' && isUtf8(file)) {
        const data = this.getUtf8Data(file);
        file.data = data;
      }
    });
  }

  /**
   * Iterate through all changed files via the given function.
   */
  forEachFile(func: (commitRev: Rev, file: FileState, path: RepoPath) => void) {
    this.stack.forEach(commit => {
      commit.files.forEach((file, path) => {
        func(commit.rev, file, path);
      });
    });
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

  /** Extract utf-8 data from a file. */
  getUtf8Data(file: FileState): string {
    if (typeof file.data === 'string') {
      return file.data;
    }
    const type = file.data.type;
    if (type === 'fileStack') {
      return unwrap(this.fileStacks.at(file.data.index)).get(file.data.rev);
    } else {
      throw new Error('getUtf8Data called on non-utf8 file.');
    }
  }

  /** Test if two files have the same data. */
  isEqualFile(a: FileState, b: FileState): boolean {
    if ((a.flags ?? '') !== (b.flags ?? '')) {
      return false;
    }
    if (isUtf8(a) && isUtf8(b)) {
      return this.getUtf8Data(a) === this.getUtf8Data(b);
    }
    // We assume base85 data is immutable, non-utf8 so they won't match utf8 data.
    if (
      typeof a.data !== 'string' &&
      typeof b.data !== 'string' &&
      a.data.type === 'base85' &&
      b.data.type === 'base85'
    ) {
      return a.data.dataBase85 === b.data.dataBase85;
    }
    return false;
  }

  // Histedit-related opeations.

  /**
   * Calculate the dependencies of revisions.
   * For example, `{5: [3, 1]}` means rev 5 depends on rev 3 and rev 1.
   *
   * This is used to detect what's reasonable when reordering and dropping
   * commits. For example, if rev 3 depends on rev 2, then rev 3 cannot be
   * moved to be an ancestor of rev 2, and rev 2 cannot be dropped alone.
   */
  calculateDepMap(): Map<Rev, Set<Rev>> {
    const depMap = new Map<Rev, Set<Rev>>(this.stack.map(c => [c.rev, new Set()]));

    const fileIdxRevToCommitRev = (fileIdx: FileStackIndex, fileRev: Rev): Rev =>
      unwrap(this.fileToCommit.get(`${fileIdx}:${fileRev}`))[0];

    // Ask FileStack for dependencies about content edits.
    this.fileStacks.forEach((fileStack, fileIdx) => {
      const fileDepMap = fileStack.calculateDepMap();
      const toCommitRev = (rev: Rev) => fileIdxRevToCommitRev(fileIdx, rev);
      // Convert file revs to commit revs.
      fileDepMap.forEach((valueFileRevs, keyFileRev) => {
        const keyCommitRev = toCommitRev(keyFileRev);
        if (keyCommitRev >= 0) {
          const set = unwrap(depMap.get(keyCommitRev));
          valueFileRevs.forEach(fileRev => {
            const rev = toCommitRev(fileRev);
            if (rev >= 0) {
              set.add(rev);
            }
          });
        }
      });
    });

    // Besides, file deletion / addition / renames also introduce dependencies.
    this.stack.forEach(commit => {
      const set = unwrap(depMap.get(commit.rev));
      commit.files.forEach((file, path) => {
        const [prevRev, prevPath, prevFile] = this.parentFile(commit.rev, path, true);
        if (prevRev >= 0 && (isAbsent(prevFile) !== isAbsent(file) || prevPath !== path)) {
          set.add(prevRev);
        }
      });
    });

    return depMap;
  }

  /** Return the single parent rev, or null. */
  singleParentRev(rev: Rev): Rev | null {
    const commit = this.stack.at(rev);
    const parents = commit?.parents;
    if (parents != null) {
      const parentRev = parents?.at(0);
      if (parentRev != null && parents.length === 1) {
        return parentRev;
      }
    }
    return null;
  }

  /**
   * Test if the commit can be folded with its parent.
   */
  canFoldDown(rev: Rev): boolean {
    if (rev <= 0 || rev >= this.stack.length) {
      return false;
    }
    const commit = this.stack[rev];
    const parentRev = this.singleParentRev(rev);
    if (parentRev == null) {
      return false;
    }
    const parent = this.stack[parentRev];
    if (commit.immutableKind !== 'none' || parent.immutableKind !== 'none') {
      return false;
    }
    // This is a bit conservative. But we're not doing complex content check for now.
    const childCount = this.stack.filter(c => c.parents.includes(parentRev)).length;
    if (childCount > 1) {
      return false;
    }
    return true;
  }

  /**
   * Drop the given `rev`.
   * The callsite should take care of `files` updates.
   */
  rewriteStackDroppingRev(rev: Rev) {
    const revMapFunc = (r: Rev) => (r < rev ? r : r - 1);
    this.stack = this.stack.filter(c => c.rev !== rev).map(c => rewriteCommitRevs(c, revMapFunc));
    // Recalculate file stacks.
    this.buildFileStacks();
  }

  /**
   * Fold the commit with its parent.
   * This should only be called when `canFoldDown(rev)` returned `true`.
   */
  foldDown(rev: Rev) {
    const commit = this.stack[rev];
    const parentRev = unwrap(this.singleParentRev(rev));
    const parent = this.stack[parentRev];
    commit.files.forEach((file, path) => {
      // Fold copyFrom. `-` means "no change".
      //
      // | grand  | direct |      |                   |
      // | parent | parent | rev  | folded (copyFrom) |
      // +--------------------------------------------+
      // | A      | A->B   | B->C | A->C   (parent)   |
      // | A      | A->B   | B    | A->B   (parent)   |
      // | A      | A->B   | -    | A->B   (parent)   |
      // | A      | A      | A->C | A->C   (rev)      |
      // | A      | -      | A->C | A->C   (rev)      |
      // | -      | B      | B->C | C      (drop)     |
      const optionalParentFile = parent.files.get(file.copyFrom ?? path);
      const copyFrom = optionalParentFile?.copyFrom ?? file.copyFrom;
      if (copyFrom != null && isAbsent(this.parentFile(parentRev, file.copyFrom ?? path)[2])) {
        // "copyFrom" is no longer valid (not existed in grand parent). Drop it.
        delete file.copyFrom;
      } else {
        file.copyFrom = copyFrom;
      }
      if (this.isEqualFile(this.parentFile(parentRev, path, false /* [1] */)[2], file)) {
        // The file changes cancel out. Remove it.
        // [1]: we need to disable following renames when comparing files for cancel-out check.
        parent.files.delete(path);
      } else {
        // Fold the change of this file.
        parent.files.set(path, file);
      }
    });

    // Fold other properties to parent.
    commit.originalNodes.forEach(node => parent.originalNodes.add(node));
    parent.date = commit.date;
    if (isMeaningfulText(commit.text)) {
      parent.text = `${parent.text.trim()}\n\n${commit.text}`;
    }

    // Update this.stack.
    this.rewriteStackDroppingRev(rev);
  }
}

function getBottomFilesFromExportStack(stack: ExportStack): Map<RepoPath, FileState> {
  // bottomFiles requires that the stack only has one root.
  checkStackSingleRoot(stack);

  // Calculate bottomFiles.
  const bottomFiles: Map<RepoPath, FileState> = new Map();
  stack.forEach(commit => {
    for (const [path, file] of Object.entries(commit.relevantFiles ?? {})) {
      if (!bottomFiles.has(path)) {
        bottomFiles.set(path, convertExportFileToFileState(file));
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

function convertExportFileToFileState(file: ExportFile | null): FileState {
  if (file == null) {
    return ABSENT_FILE;
  }
  return {
    data: file.data != null ? file.data : {type: 'base85', dataBase85: unwrap(file.dataBase85)},
    copyFrom: file.copyFrom,
    flags: file.flags,
  };
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
      Object.entries(commit.files ?? {}).map(([path, file]) => [
        path,
        convertExportFileToFileState(file),
      ]),
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

/** Rewrite fields that contains `rev` based on the mapping function. */
function rewriteCommitRevs(commit: CommitState, revMapFunc: (rev: Rev) => Rev): CommitState {
  commit.rev = revMapFunc(commit.rev);
  commit.parents = commit.parents.map(revMapFunc);
  return commit;
}

/** Guess if commit message is meaningful. Messages like "wip" or "fixup" are meaningless. */
function isMeaningfulText(text: string): boolean {
  const trimmed = text.trim();
  return trimmed.includes(' ') || trimmed.includes('\n') || trimmed.length > 20;
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
function isAbsent(file: FileState | undefined): boolean {
  if (file == null) {
    return true;
  }
  return file.flags === ABSENT_FLAG;
}

/** Test if a file has utf-8 content. */
function isUtf8(file: FileState): boolean {
  if (typeof file.data === 'string') {
    return true;
  }
  const type = file.data.type;
  return type == 'fileStack';
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
export const ABSENT_FILE: FileState = {
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
  files: Map<RepoPath, FileState>;
};

/**
 * Similar to `ExportFile` but `data` can be lazy by redirecting to a rev in a file stack.
 * Besides, supports "absent" state.
 */
type FileState = {
  data:
    | string
    | {type: 'base85'; dataBase85: string}
    | {type: 'fileStack'; index: FileStackIndex; rev: Rev};
  /** If present, this file is copied (or renamed) from another file. */
  copyFrom?: RepoPath;
  /** 'x': executable. 'l': symlink. 'm': submodule. */
  flags?: string;
};

type FileStackIndex = number;
