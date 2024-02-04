/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Dag} from './dag/dag';
import type {CommitTreeWithPreviews} from './getCommitTree';
import type {Operation} from './operations/Operation';
import type {OperationInfo, OperationList} from './serverAPIState';
import type {ChangedFile, CommitInfo, Hash, MergeConflicts, UncommittedChanges} from './types';

import {latestSuccessorsMap} from './SuccessionTracker';
import {getTracker} from './analytics/globalTracker';
import {getCommitTree, walkTreePostorder} from './getCommitTree';
import {getOpName} from './operations/Operation';
import {
  operationBeingPreviewed,
  latestCommits,
  latestCommitsData,
  latestUncommittedChangesData,
  mergeConflicts,
  latestDag,
  latestHeadCommit,
  latestUncommittedChanges,
  operationList,
  queuedOperations,
} from './serverAPIState';
import {useEffect} from 'react';
import {selector, useRecoilState, useRecoilValue} from 'recoil';
import {notEmpty, unwrap} from 'shared/utils';

export enum CommitPreview {
  REBASE_ROOT = 'rebase-root',
  REBASE_DESCENDANT = 'rebase-descendant',
  REBASE_OLD = 'rebase-old',
  REBASE_OPTIMISTIC_ROOT = 'rebase-optimistic-root',
  REBASE_OPTIMISTIC_DESCENDANT = 'rebase-optimistic-descendant',
  GOTO_DESTINATION = 'goto-destination',
  GOTO_PREVIOUS_LOCATION = 'goto-previous-location',
  HIDDEN_ROOT = 'hidden-root',
  HIDDEN_DESCENDANT = 'hidden-descendant',
  STACK_EDIT_ROOT = 'stack-edit-root',
  STACK_EDIT_DESCENDANT = 'stack-edit-descendant',
  FOLD_PREVIEW = 'fold-preview',
  FOLD = 'fold',
  // Commit being rendered in some other context than the commit tree,
  // such as the commit info sidebar
  NON_ACTIONABLE_COMMIT = 'non-actionable-commit',
}

/**
 * Alter the set of Uncommitted Changes.
 */
export type ApplyUncommittedChangesPreviewsFuncType = (
  changes: UncommittedChanges,
) => UncommittedChanges;

/**
 * Alter the set of Merge Conflicts.
 */
export type ApplyMergeConflictsPreviewsFuncType = (
  conflicts: MergeConflicts | undefined,
) => MergeConflicts | undefined;

function applyPreviewsToChangedFiles(
  files: Array<ChangedFile>,
  list: OperationList,
  queued: Array<Operation>,
): Array<ChangedFile> {
  const currentOperation = list.currentOperation;

  // gather operations from past, current, and queued commands which could have optimistic state appliers
  type Applier = (
    context: UncommittedChangesPreviewContext,
  ) => ApplyUncommittedChangesPreviewsFuncType | undefined;
  const appliersSources: Array<Applier> = [];

  // previous commands
  for (const op of list.operationHistory) {
    if (op != null && !op.hasCompletedUncommittedChangesOptimisticState) {
      if (op.operation.makeOptimisticUncommittedChangesApplier != null) {
        appliersSources.push(
          op.operation.makeOptimisticUncommittedChangesApplier.bind(op.operation),
        );
      }
    }
  }

  // currently running/last command
  if (
    currentOperation != null &&
    !currentOperation.hasCompletedUncommittedChangesOptimisticState &&
    // don't show optimistic state if we hit an error
    (currentOperation.exitCode == null || currentOperation.exitCode === 0)
  ) {
    if (currentOperation.operation.makeOptimisticUncommittedChangesApplier != null) {
      appliersSources.push(
        currentOperation.operation.makeOptimisticUncommittedChangesApplier.bind(
          currentOperation.operation,
        ),
      );
    }
  }

  // queued commands
  for (const op of queued) {
    if (op != null) {
      if (op.makeOptimisticUncommittedChangesApplier != null) {
        appliersSources.push(op.makeOptimisticUncommittedChangesApplier.bind(op));
      }
    }
  }

  // apply in order
  if (appliersSources.length) {
    let finalChanges = files;

    for (const applierSource of appliersSources) {
      const context: UncommittedChangesPreviewContext = {
        uncommittedChanges: files,
      };

      const applier = applierSource(context);
      if (applier == null) {
        continue;
      }

      finalChanges = applier(finalChanges);
    }
    return finalChanges;
  }

  return files;
}

function applyPreviewsToMergeConflicts(
  conflicts: MergeConflicts,
  list: OperationList,
  queued: Array<Operation>,
): MergeConflicts | undefined {
  const currentOperation = list.currentOperation;
  if (conflicts.state !== 'loaded') {
    return conflicts;
  }

  // gather operations from past, current, and queued commands which could have optimistic state appliers
  type Applier = (
    context: MergeConflictsPreviewContext,
  ) => ApplyMergeConflictsPreviewsFuncType | undefined;
  const appliersSources: Array<Applier> = [];

  // previous commands
  for (const op of list.operationHistory) {
    if (op != null && !op.hasCompletedMergeConflictsOptimisticState) {
      if (op.operation.makeOptimisticMergeConflictsApplier != null) {
        appliersSources.push(op.operation.makeOptimisticMergeConflictsApplier.bind(op.operation));
      }
    }
  }

  // currently running/last command
  if (
    currentOperation != null &&
    !currentOperation.hasCompletedMergeConflictsOptimisticState &&
    // don't show optimistic state if we hit an error
    (currentOperation.exitCode == null || currentOperation.exitCode === 0)
  ) {
    if (currentOperation.operation.makeOptimisticMergeConflictsApplier != null) {
      appliersSources.push(
        currentOperation.operation.makeOptimisticMergeConflictsApplier.bind(
          currentOperation.operation,
        ),
      );
    }
  }

  // queued commands
  for (const op of queued) {
    if (op != null) {
      if (op.makeOptimisticMergeConflictsApplier != null) {
        appliersSources.push(op.makeOptimisticMergeConflictsApplier.bind(op));
      }
    }
  }

  // apply in order
  if (appliersSources.length) {
    let finalChanges: MergeConflicts | undefined = conflicts;

    for (const applierSource of appliersSources) {
      const context: MergeConflictsPreviewContext = {
        conflicts,
      };

      const applier = applierSource(context);
      if (applier == null) {
        continue;
      }

      finalChanges = applier(finalChanges);
    }
    return finalChanges;
  }
  return conflicts;
}

export const uncommittedChangesWithPreviews = selector({
  key: 'uncommittedChangesWithPreviews',
  get: ({get}): Array<ChangedFile> => {
    const list = get(operationList);
    const queued = get(queuedOperations);
    const uncommittedChanges = get(latestUncommittedChanges);

    return applyPreviewsToChangedFiles(uncommittedChanges, list, queued);
  },
});

export const optimisticMergeConflicts = selector<MergeConflicts | undefined>({
  key: 'optimisticMergeConflicts',
  get: ({get}) => {
    const list = get(operationList);
    const queued = get(queuedOperations);
    const conflicts = get(mergeConflicts);
    if (conflicts?.files == null) {
      return conflicts;
    }

    return applyPreviewsToMergeConflicts(conflicts, list, queued);
  },
});

export type TreeWithPreviews = {
  trees: Array<CommitTreeWithPreviews>;
  treeMap: Map<Hash, CommitTreeWithPreviews>;
  headCommit?: CommitInfo;
};

export type WithPreviewType = {
  previewType?: CommitPreview;
  /**
   * Insertion batch. Larger: later inserted.
   * All 'sl log' commits share a same initial number.
   * Later previews might have larger numbers.
   * Used for sorting.
   */
  seqNumber?: number;
};

export type {Dag};

export const dagWithPreviews = selector<Dag>({
  key: 'dagWithPreviews',
  get: ({get}) => {
    const originalDag = get(latestDag);
    const list = get(operationList);
    const queued = get(queuedOperations);
    const currentOperation = list.currentOperation;
    const history = list.operationHistory;
    const currentPreview = get(operationBeingPreviewed);
    let dag = originalDag;
    for (const op of optimisticOperations({history, queued, currentOperation})) {
      dag = op.optimisticDag(dag);
    }
    if (currentPreview) {
      dag = currentPreview.previewDag(dag);
    }
    return dag;
  },
});

export const treeWithPreviews = selector({
  key: 'treeWithPreviews',
  get: ({get}): TreeWithPreviews => {
    const dag = get(dagWithPreviews);
    const commits = [...dag.values()];
    const trees = getCommitTree(commits);

    let headCommit = get(latestHeadCommit);
    // The headCommit might be changed by dag previews. Double check.
    if (headCommit && !dag.get(headCommit.hash)?.isHead) {
      headCommit = dag.resolve('.');
    }
    // Open-code latestCommitTreeMap to pick up tree changes done by `dag`.
    const treeMap = new Map<Hash, CommitTreeWithPreviews>();
    for (const tree of walkTreePostorder(trees)) {
      treeMap.set(tree.info.hash, tree);
    }

    return {trees, treeMap, headCommit};
  },
});

/** Yield operations that might need optimistic state. */
function* optimisticOperations(props: {
  history: OperationInfo[];
  queued: Operation[];
  currentOperation?: OperationInfo;
}): Generator<Operation> {
  const {history, queued, currentOperation} = props;

  // previous commands
  for (const op of history) {
    if (op != null && !op.hasCompletedOptimisticState) {
      yield op.operation;
    }
  }

  // currently running/last command
  if (
    currentOperation != null &&
    !currentOperation.hasCompletedOptimisticState &&
    // don't show optimistic state if we hit an error
    (currentOperation.exitCode == null || currentOperation.exitCode === 0)
  ) {
    yield currentOperation.operation;
  }

  // queued commands
  for (const op of queued) {
    if (op != null) {
      yield op;
    }
  }
}

/**
 * Mark operations as completed when their optimistic applier is no longer needed.
 * Similarly marks uncommitted changes optimistic state resolved.
 * n.b. this must be a useEffect since React doesn't like setCurrentOperation getting called during render
 * when ongoingOperation is used elsewhere in the tree
 */
export function useMarkOperationsCompleted(): void {
  const fetchedCommits = useRecoilValue(latestCommitsData);
  const commits = useRecoilValue(latestCommits);
  const uncommittedChanges = useRecoilValue(latestUncommittedChangesData);
  const conflicts = useRecoilValue(mergeConflicts);
  const successorMap = useRecoilValue(latestSuccessorsMap);

  const [list, setOperationList] = useRecoilState(operationList);

  // Mark operations as completed when their optimistic applier is no longer needed
  // n.b. this must be a useEffect since React doesn't like setCurrentOperation getting called during render
  // when ongoingOperation is used elsewhere in the tree
  useEffect(() => {
    const toMarkResolved: Array<ReturnType<typeof shouldMarkOptimisticChangesResolved>> = [];
    const uncommittedContext = {
      uncommittedChanges: uncommittedChanges.files ?? [],
    };
    const mergeConflictsContext = {
      conflicts,
    };
    const currentOperation = list.currentOperation;

    for (const operation of [...list.operationHistory, currentOperation]) {
      if (operation) {
        toMarkResolved.push(
          shouldMarkOptimisticChangesResolved(operation, uncommittedContext, mergeConflictsContext),
        );
      }
    }
    if (toMarkResolved.some(notEmpty)) {
      const operationHistory = [...list.operationHistory];
      const currentOperation =
        list.currentOperation == null ? undefined : {...list.currentOperation};
      for (let i = 0; i < toMarkResolved.length - 1; i++) {
        if (toMarkResolved[i]?.commits) {
          operationHistory[i] = {
            ...operationHistory[i],
            hasCompletedOptimisticState: true,
          };
        }
        if (toMarkResolved[i]?.files) {
          operationHistory[i] = {
            ...operationHistory[i],
            hasCompletedUncommittedChangesOptimisticState: true,
          };
        }
        if (toMarkResolved[i]?.conflicts) {
          operationHistory[i] = {
            ...operationHistory[i],
            hasCompletedMergeConflictsOptimisticState: true,
          };
        }
      }
      const markCurrentOpResolved = toMarkResolved[toMarkResolved.length - 1];
      if (markCurrentOpResolved && currentOperation != null) {
        if (markCurrentOpResolved.commits) {
          currentOperation.hasCompletedOptimisticState = true;
        }
        if (markCurrentOpResolved.files) {
          currentOperation.hasCompletedUncommittedChangesOptimisticState = true;
        }
        if (markCurrentOpResolved.conflicts) {
          currentOperation.hasCompletedMergeConflictsOptimisticState = true;
        }
      }
      setOperationList({operationHistory, currentOperation});
    }

    function shouldMarkOptimisticChangesResolved(
      operation: OperationInfo,
      uncommittedChangesContext: UncommittedChangesPreviewContext,
      mergeConflictsContext: MergeConflictsPreviewContext,
    ): {commits: boolean; files: boolean; conflicts: boolean} | undefined {
      let files = false;
      let commits = false;
      let conflicts = false;

      if (operation != null && !operation.hasCompletedUncommittedChangesOptimisticState) {
        if (operation.operation.makeOptimisticUncommittedChangesApplier != null) {
          const optimisticApplier =
            operation.operation.makeOptimisticUncommittedChangesApplier(uncommittedChangesContext);
          if (operation.exitCode != null) {
            if (optimisticApplier == null || operation.exitCode !== 0) {
              files = true;
            } else if (
              uncommittedChanges.fetchStartTimestamp > unwrap(operation.endTime).valueOf()
            ) {
              getTracker()?.track('OptimisticFilesStateForceResolved', {extras: {}});
              files = true;
            }
          }
        } else if (operation.exitCode != null) {
          files = true;
        }
      }

      if (operation != null && !operation.hasCompletedMergeConflictsOptimisticState) {
        if (operation.operation.makeOptimisticMergeConflictsApplier != null) {
          const optimisticApplier =
            operation.operation.makeOptimisticMergeConflictsApplier(mergeConflictsContext);
          if (operation.exitCode != null) {
            if (optimisticApplier == null || operation.exitCode !== 0) {
              conflicts = true;
            } else if (
              (mergeConflictsContext.conflicts?.fetchStartTimestamp ?? 0) >
              unwrap(operation.endTime).valueOf()
            ) {
              getTracker()?.track('OptimisticConflictsStateForceResolved', {
                extras: {operation: getOpName(operation.operation)},
              });
              conflicts = true;
            }
          }
        } else if (operation.exitCode != null) {
          conflicts = true;
        }
      }

      if (operation != null && !operation.hasCompletedOptimisticState) {
        const endTime = operation.endTime?.valueOf();
        if (endTime && fetchedCommits.fetchStartTimestamp >= endTime) {
          commits = true;
        }
      }

      if (commits || files || conflicts) {
        return {commits, files, conflicts};
      }
      return undefined;
    }
  }, [
    list,
    setOperationList,
    commits,
    uncommittedChanges,
    conflicts,
    fetchedCommits,
    successorMap,
  ]);
}

export type UncommittedChangesPreviewContext = {
  uncommittedChanges: UncommittedChanges;
};

export type MergeConflictsPreviewContext = {
  conflicts: MergeConflicts | undefined;
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Class<T> = new (...args: any[]) => T;
/**
 * React hook which looks in operation queue and history to see if a
 * particular operation is running or queued to run.
 * ```
 * const isRunning = useIsOperationRunningOrQueued(PullOperation);
 * ```
 */
export function useIsOperationRunningOrQueued(
  cls: Class<Operation>,
): 'running' | 'queued' | undefined {
  const list = useRecoilValue(operationList);
  const queued = useRecoilValue(queuedOperations);
  if (list.currentOperation?.operation instanceof cls && list.currentOperation?.exitCode == null) {
    return 'running';
  } else if (queued.some(op => op instanceof cls)) {
    return 'queued';
  }
  return undefined;
}

export function useMostRecentPendingOperation(): Operation | undefined {
  const list = useRecoilValue(operationList);
  const queued = useRecoilValue(queuedOperations);
  if (queued.length > 0) {
    return queued.at(-1);
  }
  if (list.currentOperation?.exitCode == null) {
    return list.currentOperation?.operation;
  }
  return undefined;
}
