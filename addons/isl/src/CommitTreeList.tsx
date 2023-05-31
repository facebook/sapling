/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitTreeWithPreviews} from './getCommitTree';
import type {Hash} from './types';
import type {ContextMenuItem} from 'shared/ContextMenu';

import {BranchIndicator} from './BranchIndicator';
import serverAPI from './ClientToServerAPI';
import {Commit} from './Commit';
import {Center, FlexRow, LargeSpinner} from './ComponentUtils';
import {ErrorNotice} from './ErrorNotice';
import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {StackEditIcon} from './StackEditIcon';
import {StackEditSubTree, UndoDescription} from './StackEditSubTree';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {allDiffSummaries, codeReviewProvider, pageVisibility} from './codeReview/CodeReviewInfo';
import {isTreeLinear, walkTreePostorder} from './getCommitTree';
import {T, t} from './i18n';
import {CreateEmptyInitialCommitOperation} from './operations/CreateEmptyInitialCommitOperation';
import {ImportStackOperation} from './operations/ImportStackOperation';
import {treeWithPreviews, useMarkOperationsCompleted} from './previews';
import {useArrowKeysToChangeSelection} from './selection';
import {
  commitFetchError,
  commitsShownRange,
  isFetchingAdditionalCommits,
  latestHeadCommit,
  latestUncommittedChangesData,
  useRunOperation,
} from './serverAPIState';
import {
  bumpStackEditMetric,
  editingStackHashes,
  loadingStackState,
  sendStackEditMetrics,
  useStackEditState,
} from './stackEditState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {ErrorShortMessages} from 'isl-server/src/constants';
import {useRecoilState, useRecoilValue, useSetRecoilState} from 'recoil';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {generatorContains, notEmpty} from 'shared/utils';

import './CommitTreeList.css';

export function CommitTreeList() {
  // Make sure we trigger subscription to changes to uncommitted changes *before* we have a tree to render,
  // so we don't miss the first returned uncommitted changes mesage.
  // TODO: This is a little ugly, is there a better way to tell recoil to start the subscription immediately?
  // Or should we queue/cache messages?
  useRecoilState(latestUncommittedChangesData);
  useRecoilState(pageVisibility);

  useMarkOperationsCompleted();

  useArrowKeysToChangeSelection();

  const {trees} = useRecoilValue(treeWithPreviews);
  const fetchError = useRecoilValue(commitFetchError);
  return fetchError == null && trees.length === 0 ? (
    <Center>
      <LargeSpinner />
    </Center>
  ) : (
    <>
      {fetchError ? <CommitFetchError error={fetchError} /> : null}
      <div
        className="commit-tree-root commit-group with-vertical-line"
        data-testid="commit-tree-root">
        <MainLineEllipsis />
        {trees.map(tree => (
          <SubTree key={tree.info.hash} tree={tree} depth={0} />
        ))}
        <MainLineEllipsis>
          <FetchingAdditionalCommitsButton />
          <FetchingAdditionalCommitsIndicator />
        </MainLineEllipsis>
      </div>
    </>
  );
}

function CommitFetchError({error}: {error: Error}) {
  const runOperation = useRunOperation();
  if (error.message === ErrorShortMessages.NoCommitsFetched) {
    return (
      <ErrorNotice
        title={t('No commits found')}
        description={t('If this is a new repository, try adding an initial commit first.')}
        error={error}
        buttons={[
          <VSCodeButton
            appearance="secondary"
            onClick={() => {
              runOperation(new CreateEmptyInitialCommitOperation());
            }}>
            <T>Create empty initial commit</T>
          </VSCodeButton>,
        ]}
      />
    );
  }
  return <ErrorNotice title={t('Failed to fetch commits')} error={error} />;
}

function SubTree({tree, depth}: {tree: CommitTreeWithPreviews; depth: number}): React.ReactElement {
  const {info, children, previewType} = tree;
  const isPublic = info.phase === 'public';

  const stackHashes = useRecoilValue(editingStackHashes);
  const loadingState = useRecoilValue(loadingStackState);
  const isStackEditing =
    depth > 0 && stackHashes.has(info.hash) && loadingState.state === 'hasValue';

  const stackActions =
    !isPublic && depth === 1 ? <StackActions key="stack-actions" tree={tree} /> : null;

  if (isStackEditing) {
    return (
      <>
        <StackEditSubTree />
        {stackActions}
      </>
    );
  }

  const renderedChildren = (children ?? [])
    .map(tree => <SubTree key={`tree-${tree.info.hash}`} tree={tree} depth={depth + 1} />)
    .map((components, i) => {
      if (!isPublic && i === 0) {
        // first child can be rendered without branching, so single-child lineages render in the same branch
        return components;
      }
      // any additional children render with branches
      return [
        <Branch key={`branch-${info.hash}-${i}`} descendsFrom={info.hash}>
          {components}
        </Branch>,
      ];
    })
    .flat();

  const rendered = [
    ...renderedChildren,
    <Commit
      commit={info}
      key={info.hash}
      previewType={previewType}
      hasChildren={renderedChildren.length > 0}
    />,
    stackActions,
  ].filter(notEmpty);

  return <>{rendered}</>;
}

function Branch({
  children,
  descendsFrom,
  className,
}: {
  children: React.ReactElement;
  descendsFrom: Hash;
  className?: string;
}) {
  return (
    <div
      className={`commit-group ${className ?? 'with-vertical-line'}`}
      data-testid={`branch-from-${descendsFrom}`}>
      {children}
      <BranchIndicator />
    </div>
  );
}

/**
 * Vertical ellipsis to be rendered on top of the branch line.
 * Expects to rendered as a child of commit-tree-root.
 * Optionally accepts children to render next to the "..."
 */
function MainLineEllipsis({children}: {children?: React.ReactNode}) {
  return (
    <div className="commit-ellipsis">
      <Icon icon="kebab-vertical" />
      <div className="commit-ellipsis-children">{children}</div>
    </div>
  );
}

function FetchingAdditionalCommitsIndicator() {
  const isFetching = useRecoilValue(isFetchingAdditionalCommits);
  return isFetching ? <Icon icon="loading" /> : null;
}

function FetchingAdditionalCommitsButton() {
  const shownRange = useRecoilValue(commitsShownRange);
  const isFetching = useRecoilValue(isFetchingAdditionalCommits);
  if (shownRange === undefined) {
    return null;
  }
  const commitsShownMessage = t('Showing comits from the last $numDays days', {
    replace: {$numDays: shownRange.toString()},
  });
  return (
    <Tooltip placement="top" delayMs={DOCUMENTATION_DELAY} title={commitsShownMessage}>
      <VSCodeButton
        disabled={isFetching}
        onClick={() => {
          serverAPI.postMessage({
            type: 'loadMoreCommits',
          });
        }}
        appearance="icon">
        <T>Load more commits</T>
      </VSCodeButton>
    </Tooltip>
  );
}

function StackActions({tree}: {tree: CommitTreeWithPreviews}): React.ReactElement | null {
  const reviewProvider = useRecoilValue(codeReviewProvider);
  const diffMap = useRecoilValue(allDiffSummaries);
  const stackHashes = useRecoilValue(editingStackHashes);
  const loadingState = useRecoilValue(loadingStackState);
  const runOperation = useRunOperation();

  // buttons at the bottom of the stack
  const actions = [];
  // additional actions hidden behind [...] menu.
  // Non-empty only when actions is non-empty.
  const moreActions: Array<ContextMenuItem> = [];

  const isStackEditingActivated =
    stackHashes.size > 0 &&
    loadingState.state === 'hasValue' &&
    generatorContains(walkTreePostorder([tree]), v => stackHashes.has(v.info.hash));

  const contextMenu = useContextMenu(() => moreActions);
  if (reviewProvider !== null && !isStackEditingActivated) {
    const reviewActions =
      diffMap.value == null ? {} : reviewProvider?.getSupportedStackActions(tree, diffMap.value);
    const resubmittableStack = reviewActions?.resubmittableStack;
    const submittableStack = reviewActions?.submittableStack;
    const MIN_STACK_SIZE_TO_SUGGEST_SUBMIT = 2; // don't show "submit stack" on single commits... they're not really "stacks".

    // any existing diffs -> show resubmit stack,
    if (
      resubmittableStack != null &&
      resubmittableStack.length >= MIN_STACK_SIZE_TO_SUGGEST_SUBMIT
    ) {
      actions.push(
        <HighlightCommitsWhileHovering key="resubmit-stack" toHighlight={resubmittableStack}>
          <VSCodeButton
            appearance="icon"
            onClick={() => {
              runOperation(reviewProvider.submitOperation(resubmittableStack));
            }}>
            <Icon icon="cloud-upload" slot="start" />
            <T>Resubmit stack</T>
          </VSCodeButton>
        </HighlightCommitsWhileHovering>,
      );
      //     any non-submitted diffs -> "submit all commits this stack" in hidden group
      if (
        submittableStack != null &&
        submittableStack.length > 0 &&
        submittableStack.length > resubmittableStack.length
      ) {
        moreActions.push({
          label: (
            <HighlightCommitsWhileHovering key="submit-entire-stack" toHighlight={submittableStack}>
              <FlexRow>
                <Icon icon="cloud-upload" slot="start" />
                <T>Submit entire stack</T>
              </FlexRow>
            </HighlightCommitsWhileHovering>
          ),
          onClick: () => {
            runOperation(
              reviewProvider.submitOperation([...resubmittableStack, ...submittableStack]),
            );
          },
        });
      }
      //     NO non-submitted diffs -> nothing in hidden group
    } else if (
      submittableStack != null &&
      submittableStack.length >= MIN_STACK_SIZE_TO_SUGGEST_SUBMIT
    ) {
      // NO existing diffs -> show submit stack ()
      actions.push(
        <HighlightCommitsWhileHovering key="submit-stack" toHighlight={submittableStack}>
          <VSCodeButton
            appearance="icon"
            onClick={() => {
              runOperation(reviewProvider.submitOperation(submittableStack));
            }}>
            <Icon icon="cloud-upload" slot="start" />
            <T>Submit stack</T>
          </VSCodeButton>
        </HighlightCommitsWhileHovering>,
      );
    }
  }

  if (tree.children.length > 0) {
    actions.push(<StackEditButton key="edit-stack" tree={tree} />);
  }

  if (actions.length === 0) {
    return null;
  }
  const moreActionsButton =
    moreActions.length === 0 ? null : (
      <VSCodeButton key="more-actions" appearance="icon" onClick={contextMenu}>
        <Icon icon="ellipsis" />
      </VSCodeButton>
    );
  return (
    <div className="commit-tree-stack-actions">
      {actions}
      {moreActionsButton}
    </div>
  );
}

function StackEditConfirmButtons(): React.ReactElement {
  const setStackHashes = useSetRecoilState(editingStackHashes);
  const originalHead = useRecoilValue(latestHeadCommit);
  const runOperation = useRunOperation();
  const stackEdit = useStackEditState();

  const canUndo = stackEdit.canUndo();
  const canRedo = stackEdit.canRedo();

  const handleUndo = () => {
    stackEdit.undo();
    bumpStackEditMetric('undo');
  };

  const handleRedo = () => {
    stackEdit.redo();
    bumpStackEditMetric('redo');
  };

  const handleSaveChanges = () => {
    const importStack = stackEdit.commitStack.calculateImportStack({
      goto: originalHead?.hash,
      rewriteDate: Date.now() / 1000,
    });
    const op = new ImportStackOperation(importStack);
    runOperation(op);
    sendStackEditMetrics(true);
    // Exit stack editing.
    setStackHashes(new Set());
  };

  const handleCancel = () => {
    sendStackEditMetrics(false);
    setStackHashes(new Set<Hash>());
  };

  // Show [Cancel] [Save changes] [Undo] [Redo].
  return (
    <>
      <Tooltip
        title={t('Discard stack editing changes')}
        delayMs={DOCUMENTATION_DELAY}
        placement="bottom">
        <VSCodeButton
          className="cancel-edit-stack-button"
          appearance="secondary"
          onClick={handleCancel}>
          <T>Cancel</T>
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        title={t('Save stack editing changes')}
        delayMs={DOCUMENTATION_DELAY}
        placement="bottom">
        <VSCodeButton
          className="confirm-edit-stack-button"
          appearance="primary"
          onClick={handleSaveChanges}>
          <T>Save changes</T>
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        component={() =>
          canUndo ? (
            <T replace={{$op: <UndoDescription op={stackEdit.undoOperationDescription()} />}}>
              Undo $op
            </T>
          ) : (
            <T>No operations to undo</T>
          )
        }
        placement="bottom">
        <VSCodeButton appearance="icon" disabled={!canUndo} onClick={handleUndo}>
          <Icon icon="discard" />
        </VSCodeButton>
      </Tooltip>
      <Tooltip
        component={() =>
          canRedo ? (
            <T replace={{$op: <UndoDescription op={stackEdit.redoOperationDescription()} />}}>
              Redo $op
            </T>
          ) : (
            <T>No operations to redo</T>
          )
        }
        placement="bottom">
        <VSCodeButton appearance="icon" disabled={!canRedo} onClick={handleRedo}>
          <Icon icon="redo" />
        </VSCodeButton>
      </Tooltip>
    </>
  );
}

function StackEditButton({tree}: {tree: CommitTreeWithPreviews}): React.ReactElement | null {
  const uncommitted = useRecoilValue(latestUncommittedChangesData);
  const [stackHashes, setStackHashes] = useRecoilState(editingStackHashes);
  const loadingState = useRecoilValue(loadingStackState);

  const stackCommits = [...walkTreePostorder([tree])].map(t => t.info);
  const isEditing = stackHashes.size > 0 && stackCommits.some(c => stackHashes.has(c.hash));
  const isLoaded = isEditing && loadingState.state === 'hasValue';
  if (isLoaded) {
    return <StackEditConfirmButtons />;
  }

  const isPreview = tree.previewType != null;
  const isLoading = isEditing && loadingState.state === 'loading';
  const isError = isEditing && loadingState.state === 'hasError';
  const isLinear = isTreeLinear(tree);
  const isDirty = stackCommits.some(c => c.isHead) && uncommitted.files.length > 0;
  const hasPublic = stackCommits.some(c => c.phase === 'public');
  const obsoleted = stackCommits.filter(c => c.successorInfo != null);
  const hasObsoleted = obsoleted.length > 0;
  const disabled =
    isDirty || hasObsoleted || !isLinear || isLoading || isError || isPreview || hasPublic;
  const title = isError
    ? t(`Failed to load stack: ${loadingState.error}`)
    : isLoading
    ? loadingState.exportedStack === undefined
      ? t('Reading stack content')
      : t('Analyzing stack content')
    : hasObsoleted
    ? t('Cannot edit stack with commits that have newer versions')
    : isDirty
    ? t(
        'Cannot edit stack when there are uncommitted changes.\nCommit or amend your changes first.',
      )
    : isPreview
    ? t('Cannot edit pending changes')
    : hasPublic
    ? t('Cannot edit public commits')
    : isLinear
    ? t('Reorder, fold, or drop commits')
    : t('Cannot edit non-linear stack');
  const highlight = disabled ? [] : stackCommits;
  const tooltipDelay = disabled && !isLoading ? undefined : DOCUMENTATION_DELAY;
  const icon = isLoading ? <Icon icon="loading" slot="start" /> : <StackEditIcon slot="start" />;

  return (
    <HighlightCommitsWhileHovering key="submit-stack" toHighlight={highlight}>
      <Tooltip title={title} delayMs={tooltipDelay} placement="bottom">
        <VSCodeButton
          className={`edit-stack-button ${disabled && 'disabled'}`}
          disabled={disabled}
          appearance="icon"
          onClick={() => {
            setStackHashes(new Set<Hash>(stackCommits.map(c => c.hash)));
          }}>
          {icon}
          <T>Edit stack</T>
        </VSCodeButton>
      </Tooltip>
    </HighlightCommitsWhileHovering>
  );
}
