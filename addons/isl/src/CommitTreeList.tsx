/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {UICodeReviewProvider} from './codeReview/UICodeReviewProvider';
import type {CommitTree, CommitTreeWithPreviews} from './getCommitTree';
import type {CommitInfo, DiffSummary, Hash} from './types';
import type {ContextMenuItem} from 'shared/ContextMenu';

import {BranchIndicator} from './BranchIndicator';
import serverAPI from './ClientToServerAPI';
import {Commit} from './Commit';
import {Center, FlexRow, LargeSpinner} from './ComponentUtils';
import {ErrorNotice} from './ErrorNotice';
import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {OperationDisabledButton} from './OperationDisabledButton';
import {StackEditConfirmButtons} from './StackEditConfirmButtons';
import {StackEditIcon} from './StackEditIcon';
import {StackEditSubTree} from './StackEditSubTree';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {allDiffSummaries, codeReviewProvider, pageVisibility} from './codeReview/CodeReviewInfo';
import {isTreeLinear, walkTreePostorder} from './getCommitTree';
import {T, t} from './i18n';
import {CreateEmptyInitialCommitOperation} from './operations/CreateEmptyInitialCommitOperation';
import {HideOperation} from './operations/HideOperation';
import {treeWithPreviews, useMarkOperationsCompleted} from './previews';
import {useArrowKeysToChangeSelection} from './selection';
import {
  commitFetchError,
  commitsShownRange,
  isFetchingAdditionalCommits,
  latestUncommittedChangesData,
  useRunOperation,
} from './serverAPIState';
import {editingStackHashes, loadingStackState} from './stackEditState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {ErrorShortMessages} from 'isl-server/src/constants';
import {useRecoilState, useRecoilValue} from 'recoil';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {generatorContains, notEmpty, unwrap} from 'shared/utils';

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
      {trees.length === 0 ? null : (
        <div
          className="commit-tree-root commit-group with-vertical-line"
          data-testid="commit-tree-root">
          <MainLineEllipsis />
          {trees.filter(shouldShowPublicCommit).map(tree => (
            <SubTree key={tree.info.hash} tree={tree} depth={0} />
          ))}
          <MainLineEllipsis>
            <FetchingAdditionalCommitsButton />
            <FetchingAdditionalCommitsIndicator />
          </MainLineEllipsis>
        </div>
      )}
    </>
  );
}

/**
 * Ensure only relevant public commits are shown.
 * `sl log` does this kind of filtering for us anyway, but
 * if a commit is hidden due to previews or optimistic state,
 * we can violate these conditions.
 */
function shouldShowPublicCommit(tree: CommitTree) {
  return (
    tree.info.isHead ||
    tree.children.length > 0 ||
    tree.info.bookmarks.length > 0 ||
    tree.info.remoteBookmarks.length > 0 ||
    (tree.info.stableCommitMetadata?.length ?? 0) > 0
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

function isStackEligibleForCleanup(
  tree: CommitTreeWithPreviews,
  diffMap: Map<string, DiffSummary>,
  provider: UICodeReviewProvider,
): boolean {
  if (
    tree.info.diffId == null ||
    tree.info.isHead || // don't allow hiding a stack you're checked out on
    diffMap.get(tree.info.diffId) == null ||
    !provider.isDiffEligibleForCleanup(unwrap(diffMap.get(tree.info.diffId)))
  ) {
    return false;
  }

  // any child not eligible -> don't show
  for (const subtree of tree.children) {
    if (!isStackEligibleForCleanup(subtree, diffMap, provider)) {
      return false;
    }
  }

  return true;
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

  const showCleanupButton =
    reviewProvider == null || diffMap?.value == null
      ? false
      : isStackEligibleForCleanup(tree, diffMap.value, reviewProvider);

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
          <OperationDisabledButton
            // Use the diffId in the key so that only this "resubmit stack" button shows the spinner.
            contextKey={`resubmit-stack-on-${tree.info.diffId}`}
            appearance="icon"
            icon={<Icon icon="cloud-upload" slot="start" />}
            runOperation={() => {
              return reviewProvider.submitOperation(resubmittableStack);
            }}>
            <T>Resubmit stack</T>
          </OperationDisabledButton>
        </HighlightCommitsWhileHovering>,
      );
      // any non-submitted diffs -> "submit all commits this stack" in hidden group
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
      // NO non-submitted diffs -> nothing in hidden group
    } else if (
      submittableStack != null &&
      submittableStack.length >= MIN_STACK_SIZE_TO_SUGGEST_SUBMIT
    ) {
      // We need to associate this operation with the stack we're submitting,
      // but during submitting, we'll amend the original commit, so hash is not accurate.
      // Parent is close, but if you had multiple stacks rebased to the same public commit,
      // all those stacks would render the same key and show the same spinner.
      // So parent hash + title heuristic lets us almost always show the spinner for only this stack.
      const contextKey = `submit-stack-on-${tree.info.parents[0]}-${tree.info.title.replace(
        / /g,
        '_',
      )}`;
      // NO existing diffs -> show submit stack ()
      actions.push(
        <HighlightCommitsWhileHovering key="submit-stack" toHighlight={submittableStack}>
          <OperationDisabledButton
            contextKey={contextKey}
            appearance="icon"
            icon={<Icon icon="cloud-upload" slot="start" />}
            runOperation={() => {
              return reviewProvider.submitOperation(submittableStack);
            }}>
            <T>Submit stack</T>
          </OperationDisabledButton>
        </HighlightCommitsWhileHovering>,
      );
    }
  }

  if (tree.children.length > 0) {
    actions.push(<StackEditButton key="edit-stack" tree={tree} />);
  }

  if (showCleanupButton) {
    actions.push(
      <CleanupButton key="cleanup" commit={tree.info} hasChildren={tree.children.length > 0} />,
    );
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
    <div className="commit-tree-stack-actions" data-testid="commit-tree-stack-actions">
      {actions}
      {moreActionsButton}
    </div>
  );
}

function CleanupButton({commit, hasChildren}: {commit: CommitInfo; hasChildren: boolean}) {
  const runOperation = useRunOperation();
  return (
    <Tooltip
      title={
        hasChildren
          ? t('You can safely "clean up" by hiding this stack of commits.')
          : t('You can safely "clean up" by hiding this commit.')
      }
      placement="bottom">
      <VSCodeButton
        appearance="icon"
        onClick={() => {
          runOperation(new HideOperation(commit.hash));
        }}>
        <Icon icon="eye-closed" slot="start" />
        {hasChildren ? <T>Clean up stack</T> : <T>Clean up</T>}
      </VSCodeButton>
    </Tooltip>
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
