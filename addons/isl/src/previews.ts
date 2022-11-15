/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTree, CommitTreeWithPreviews} from './getCommitTree';
import type {Operation} from './operations/Operation';
import type {OperationInfo} from './serverAPIState';
import type {ChangedFile, CommitInfo, Hash, UncommittedChanges} from './types';

import {
  latestCommitTree,
  latestCommitTreeMap,
  latestHeadCommit,
  latestUncommittedChanges,
  operationList,
  queuedOperations,
} from './serverAPIState';
import {useEffect} from 'react';
import {atom, selector, useRecoilState, useRecoilValue} from 'recoil';
import {notEmpty} from 'shared/utils';

export enum CommitPreview {
  REBASE_ROOT = 'rebase-root',
  REBASE_DESCENDANT = 'rebase-descendant',
  REBASE_OLD = 'rebase-old',
  REBASE_OPTIMISTIC_ROOT = 'rebase-optimistic-root',
  REBASE_OPTIMISTIC_DESCENDANT = 'rebase-optimistic-descendant',
  GOTO_DESTINATION = 'goto-destination',
  GOTO_PREVIOUS_LOCATION = 'goto-previous-location',
}

/**
 * A preview applier function provides a way of iterating the tree of commits and modify it & mark commits as being in a preview state.
 * Given a commit in the tree, you can overwrite what children to render and what preview type to set.
 * This function is used to walk the entire tree and alter how it would be rendered without needing to make a copy of the entire tree that needs to be mutated.
 * The preview type is used when rendering the commit to de-emphasize it, put it in a green color, add a confirm rebase button, etc.
 * If the preview applier returns null, it means to hide the commit and all of its children.
 *
 * Preview Appliers may also be called on individual commits (as opposed to walking the entire tree)
 * when rendering their details in the Commit Info View. As such, they should not depend on being called in
 * any specific order of commits.
 */
export type ApplyPreviewsFuncType = (
  tree: CommitTree,
  previewType: CommitPreview | undefined,
  childPreviewType?: CommitPreview | undefined,
) => (
  | {
      info: null;
      children?: undefined;
    }
  | {info: CommitInfo; children: Array<CommitTree>}
) & {
  previewType?: CommitPreview;
  childPreviewType?: CommitPreview;
};

/**
 * Like ApplyPreviewsFuncType, this provides a way to alter the set of Uncommitted Changes.
 */
export type ApplyUncommittedChangesPreviewsFuncType = (
  changes: UncommittedChanges,
) => UncommittedChanges;

export const operationBeingPreviewed = atom<Operation | undefined>({
  key: 'operationBeingPreviewed',
  default: undefined,
});

export const uncommittedChangesWithPreviews = selector({
  key: 'uncommittedChangesWithPreviews',
  get: ({get}): Array<ChangedFile> => {
    const list = get(operationList);
    const queued = get(queuedOperations);
    const uncommittedChanges = get(latestUncommittedChanges);
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
      let finalChanges = uncommittedChanges;

      for (const applierSource of appliersSources) {
        const context: UncommittedChangesPreviewContext = {
          uncommittedChanges,
        };

        const applier = applierSource(context);
        if (applier == null) {
          continue;
        }

        finalChanges = applier(finalChanges);
      }
      return finalChanges;
    }

    return uncommittedChanges;
  },
});

export const treeWithPreviews = selector({
  key: 'treeWithPreviews',
  get: ({
    get,
  }): {trees: Array<CommitTree>; treeMap: Map<Hash, CommitTree>; headCommit?: CommitInfo} => {
    const trees = get(latestCommitTree);

    // gather operations from past, current, and queued commands which could have optimistic state appliers
    type Applier = (context: PreviewContext) => ApplyPreviewsFuncType | undefined;
    const appliersSources: Array<Applier> = [];

    // preview applier can either come from an operation being previewed...
    const currentPreview = get(operationBeingPreviewed);
    // ...or from an operation that is being run right now
    // or ran recently and is still showing optimistic state while waiting for new commits or uncommitted changes
    const list = get(operationList);
    const queued = get(queuedOperations);
    const currentOperation = list.currentOperation;

    // previous commands
    for (const op of list.operationHistory) {
      if (op != null && !op.hasCompletedOptimisticState) {
        if (op.operation.makeOptimisticApplier != null) {
          appliersSources.push(op.operation.makeOptimisticApplier.bind(op.operation));
        }
      }
    }

    // currently running/last command
    if (
      currentOperation != null &&
      !currentOperation.hasCompletedOptimisticState &&
      // don't show optimistic state if we hit an error
      (currentOperation.exitCode == null || currentOperation.exitCode === 0)
    ) {
      if (currentOperation.operation.makeOptimisticApplier != null) {
        appliersSources.push(
          currentOperation.operation.makeOptimisticApplier.bind(currentOperation.operation),
        );
      }
    }

    // queued commands
    for (const op of queued) {
      if (op != null) {
        if (op.makeOptimisticApplier != null) {
          appliersSources.push(op.makeOptimisticApplier.bind(op));
        }
      }
    }

    // operation being previewed (would be queued next)
    if (currentPreview?.makePreviewApplier != null) {
      appliersSources.push(currentPreview.makePreviewApplier.bind(currentPreview));
    }

    let headCommit = get(latestHeadCommit);
    let treeMap = get(latestCommitTreeMap);

    // apply in order
    if (appliersSources.length) {
      let finalTrees = trees;

      for (const applierSource of appliersSources) {
        const context: PreviewContext = {
          trees: finalTrees,
          headCommit,
          treeMap,
        };
        let nextHeadCommit = headCommit;
        const nextTreeMap = new Map<Hash, CommitTree>();

        const applier = applierSource(context);
        if (applier == null) {
          continue;
        }

        const processTree = (
          tree: CommitTreeWithPreviews,
          inheritedPreviewType?: CommitPreview,
        ): CommitTreeWithPreviews | undefined => {
          const result = applier(tree, inheritedPreviewType);
          if (result?.info == null) {
            return undefined;
          }
          if (result.info.isHead) {
            nextHeadCommit = result.info;
          }
          const {info, children, previewType, childPreviewType} = result;
          const newTree = {
            info,
            previewType,
            children: children
              .map(child => processTree(child, childPreviewType))
              .filter((tree): tree is CommitTreeWithPreviews => tree != null),
          };

          nextTreeMap.set(newTree.info.hash, result);
          return newTree;
        };
        finalTrees = finalTrees.map(tree => processTree(tree)).filter(notEmpty);
        headCommit = nextHeadCommit;
        treeMap = nextTreeMap;
      }
      return {trees: finalTrees, treeMap, headCommit};
    }

    return {trees, treeMap, headCommit};
  },
});

/**
 * Mark operations as completed when their optimistic applier is no longer needed.
 * Similarly marks uncommitted changes optimistic state resolved.
 * n.b. this must be a useEffect since React doesn't like setCurrentOperation getting called during render
 * when ongoingOperation is used elsewhere in the tree
 */
export function useMarkOperationsCompleted(): void {
  // TODO: re-write this using treeWithPreviews.
  // `treeWithPreviews` should make the determination of which operations
  // should be marked as completed, so we don't duplicate the
  // traversal of the previews here and it already knows everything about previews.
  const trees = useRecoilValue(latestCommitTree);
  const headCommit = useRecoilValue(latestHeadCommit);
  const treeMap = useRecoilValue(latestCommitTreeMap);
  const uncommittedChanges = useRecoilValue(latestUncommittedChanges);

  const [list, setOperationList] = useRecoilState(operationList);

  // Mark operations as completed when their optimistic applier is no longer needed
  // n.b. this must be a useEffect since React doesn't like setCurrentOperation getting called during render
  // when ongoingOperation is used elsewhere in the tree
  useEffect(() => {
    const toMarkResolved: Array<ReturnType<typeof shouldMarkOptimisticChangesResolved>> = [];
    const context = {
      trees,
      headCommit,
      treeMap,
    };
    const uncommittedContext = {
      uncommittedChanges,
    };
    const currentOperation = list.currentOperation;

    for (const operation of [...list.operationHistory, currentOperation]) {
      toMarkResolved.push(
        operation
          ? shouldMarkOptimisticChangesResolved(operation, context, uncommittedContext)
          : undefined,
      );
    }
    if (toMarkResolved.some(Boolean)) {
      const operationHistory = [...list.operationHistory];
      const currentOperation =
        list.currentOperation == null ? undefined : {...list.currentOperation};
      for (let i = 0; i < toMarkResolved.length - 1; i++) {
        if (toMarkResolved[i] == 'commits' || toMarkResolved[i] === 'both') {
          operationHistory[i] = {
            ...operationHistory[i],
            hasCompletedOptimisticState: true,
          };
        }
        if (toMarkResolved[i] == 'files' || toMarkResolved[i] === 'both') {
          operationHistory[i] = {
            ...operationHistory[i],
            hasCompletedUncommittedChangesOptimisticState: true,
          };
        }
      }
      const markCurrentOpResolved = toMarkResolved[toMarkResolved.length - 1];
      if (markCurrentOpResolved && currentOperation != null) {
        if (markCurrentOpResolved == 'commits' || markCurrentOpResolved === 'both') {
          currentOperation.hasCompletedOptimisticState = true;
        }
        if (markCurrentOpResolved == 'files' || markCurrentOpResolved === 'both') {
          currentOperation.hasCompletedUncommittedChangesOptimisticState = true;
        }
      }
      setOperationList({operationHistory, currentOperation});
    }

    function shouldMarkOptimisticChangesResolved(
      operation: OperationInfo,
      context: PreviewContext,
      uncommittedChangesContext: UncommittedChangesPreviewContext,
    ): 'commits' | 'files' | 'both' | undefined {
      let files = false;
      let commits = false;

      if (operation != null && !operation.hasCompletedUncommittedChangesOptimisticState) {
        if (operation.operation.makeOptimisticUncommittedChangesApplier != null) {
          const optimisticApplier =
            operation.operation.makeOptimisticUncommittedChangesApplier(uncommittedChangesContext);
          if (
            operation.exitCode != null &&
            (optimisticApplier == null || operation.exitCode !== 0)
          ) {
            files = true;
          }
        } else if (operation.exitCode != null) {
          files = true;
        }
      }

      if (operation != null && !operation.hasCompletedOptimisticState) {
        if (operation.operation.makeOptimisticApplier != null) {
          const optimisticApplier = operation.operation.makeOptimisticApplier(context);
          if (
            operation.exitCode != null &&
            (optimisticApplier == null || operation.exitCode !== 0)
          ) {
            commits = true;
          }
        } else if (operation.exitCode != null) {
          commits = true;
        }
      }

      if (files && commits) {
        return 'both';
      } else if (files) {
        return 'files';
      } else if (commits) {
        return 'commits';
      }
      return undefined;
    }
  }, [list, setOperationList, headCommit, trees, treeMap, uncommittedChanges]);
}

/** Set of info about commit tree to generate appropriate previews */
export type PreviewContext = {
  trees: Array<CommitTree>;
  headCommit?: CommitInfo;
  treeMap: Map<string, CommitTree>;
};

export type UncommittedChangesPreviewContext = {
  uncommittedChanges: UncommittedChanges;
};
