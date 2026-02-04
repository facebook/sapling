/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RenderGlyphResult} from './RenderDag';
import type {DagCommitInfo} from './dag/dag';
import type {ExtendedGraphRow} from './dag/render';
import type {Hash} from './types';

import {Button} from 'isl-components/Button';
import {ErrorNotice} from 'isl-components/ErrorNotice';
import {ErrorShortMessages} from 'isl-server/src/constants';
import {atom, useAtomValue} from 'jotai';
import {Commit, InlineProgressSpan, isExternalCommitByDiffId} from './Commit';
import {FetchingAdditionalCommitsRow} from './FetchAdditionalCommitsButton';
import {isHighlightedCommit} from './HighlightedCommits';
import {RegularGlyph, RenderDag, YouAreHereGlyph} from './RenderDag';
import {StackActions, collapsedStacksAtom} from './StackActions';
import {YOU_ARE_HERE_VIRTUAL_COMMIT} from './dag/virtualCommit';
import {T, t} from './i18n';
import {atomFamilyWeak, localStorageBackedAtom} from './jotaiUtils';
import {CreateEmptyInitialCommitOperation} from './operations/CreateEmptyInitialCommitOperation';
import {inlineProgressByHash, useRunOperation} from './operationsState';
import {dagWithPreviews, treeWithPreviews, useMarkOperationsCompleted} from './previews';
import {hideIrrelevantCwdStacks, isIrrelevantToCwd, repoRelativeCwd} from './repositoryData';
import {isNarrowCommitTree} from './responsive';
import {
  selectedCommits,
  useArrowKeysToChangeSelection,
  useBackspaceToHideSelected,
  useCommitCallbacks,
  useShortcutToRebaseSelected,
} from './selection';
import {useEffect} from 'react';
import {commitFetchError, latestUncommittedChangesData} from './serverAPIState';
import {MaybeEditStackModal} from './stackEdit/ui/EditStackModal';

import './CommitTreeList.css';

/**
 * Check if a commit has origin/main or origin/master bookmark.
 * Used to apply special highlighting to the main branch marker.
 */
function isOriginMain(commit: DagCommitInfo): boolean {
  return commit.remoteBookmarks.some(
    bookmark =>
      bookmark === 'origin/main' ||
      bookmark === 'origin/master' ||
      bookmark === 'remote/main' ||
      bookmark === 'remote/master',
  );
}

type DagCommitListProps = {
  isNarrow: boolean;
};

const dagWithYouAreHere = atom(get => {
  let dag = get(dagWithPreviews);
  // Insert a virtual "You are here" as a child of ".".
  const dot = dag.resolve('.');
  if (dot != null) {
    dag = dag.add([YOU_ARE_HERE_VIRTUAL_COMMIT.set('parents', [dot.hash])]);
  }
  return dag;
});

export const condenseObsoleteStacks = localStorageBackedAtom<boolean | null>(
  'isl.condense-obsolete-stacks',
  true,
);

const renderSubsetUnionSelection = atom(get => {
  const dag = get(dagWithYouAreHere);
  const condense = get(condenseObsoleteStacks);
  let subset = dag.subsetForRendering(undefined, /* condenseObsoleteStacks */ condense !== false);
  // If selectedCommits includes commits unknown to dag (ex. in tests), ignore them to avoid errors.
  const selection = dag.present(get(selectedCommits));

  const hideIrrelevant = get(hideIrrelevantCwdStacks);
  if (hideIrrelevant) {
    const cwd = get(repoRelativeCwd);
    subset = dag.filter(commit => commit.isDot || !isIrrelevantToCwd(commit, cwd), subset);
  }

  // Filter out commits in collapsed stacks (but keep the stack root)
  const collapsedStacks = get(collapsedStacksAtom);
  if (collapsedStacks.length > 0) {
    const collapsedSet = new Set(collapsedStacks);
    subset = dag.filter(commit => {
      // Always show the commit if it's the stack root (in collapsedStacks)
      if (collapsedSet.has(commit.hash)) {
        return true;
      }
      // Hide commits whose ancestors include a collapsed stack root
      for (const rootHash of collapsedStacks) {
        const rootCommit = dag.get(rootHash);
        if (rootCommit && dag.isAncestor(rootHash, commit.hash)) {
          // This commit is a descendant of a collapsed stack root, hide it
          return false;
        }
      }
      return true;
    }, subset);
  }

  return subset.union(selection);
});

function DagCommitList(props: DagCommitListProps) {
  const {isNarrow} = props;

  const dag = useAtomValue(dagWithYouAreHere);
  const subset = useAtomValue(renderSubsetUnionSelection);

  return (
    <RenderDag
      dag={dag}
      subset={subset}
      className={'commit-tree-root ' + (isNarrow ? ' commit-tree-narrow' : '')}
      data-testid="commit-tree-root"
      renderCommit={renderCommit}
      renderCommitExtras={renderCommitExtras}
      renderGlyph={renderGlyph}
      useExtraCommitRowProps={useExtraCommitRowProps}
      useLineColor={useLineColor}
    />
  );
}

function renderCommit(info: DagCommitInfo) {
  return <DagCommitBody info={info} />;
}

function renderCommitExtras(info: DagCommitInfo, row: ExtendedGraphRow) {
  if (row.termLine != null && (info.parents.length > 0 || (info.ancestors?.size ?? 0) > 0)) {
    // Root (no parents) in the displayed DAG, but not root in the full DAG.
    return <MaybeFetchingAdditionalCommitsRow hash={info.hash} />;
  } else if (info.phase === 'draft') {
    // Draft but parents are not drafts. Likely a stack root. Show stack buttons.
    return <MaybeStackActions hash={info.hash} />;
  }
  return null;
}

function renderGlyph(info: DagCommitInfo): RenderGlyphResult {
  if (info.isYouAreHere) {
    return ['replace-tile', <YouAreHereGlyphWithProgress info={info} />];
  } else {
    return ['inside-tile', <HighlightedGlyph info={info} />];
  }
}

function useExtraCommitRowProps(info: DagCommitInfo): React.HTMLAttributes<HTMLDivElement> | void {
  const {isSelected, onClickToSelect, onDoubleClickToShowDrawer} = useCommitCallbacks(info);

  return {
    onClick: onClickToSelect,
    onDoubleClick: onDoubleClickToShowDrawer,
    className: isSelected ? 'commit-row-selected' : '',
  };
}

const EXTERNAL_COMMIT_COLOR = 'var(--external-commit-line-color, #6b7280)';

function useLineColor(info: DagCommitInfo): string | undefined {
  const diffId = info.diffId;
  // Use empty string as sentinel for "no diffId" - atom will return false for it
  const isExternal = useAtomValue(isExternalCommitByDiffId(diffId ?? ''));
  // Only return external color if there's actually a diffId
  if (diffId == null) {
    return undefined;
  }
  return isExternal ? EXTERNAL_COMMIT_COLOR : undefined;
}

function YouAreHereGlyphWithProgress({info}: {info: DagCommitInfo}) {
  const inlineProgress = useAtomValue(inlineProgressByHash(info.hash));
  return (
    <YouAreHereGlyph info={info}>
      {inlineProgress && <InlineProgressSpan message={inlineProgress} />}
    </YouAreHereGlyph>
  );
}

const dagHasChildren = atomFamilyWeak((key: string) => {
  return atom(get => {
    const dag = get(dagWithPreviews);
    return dag.children(key).size > 0;
  });
});

function DagCommitBody({info}: {info: DagCommitInfo}) {
  const hasChildren = useAtomValue(dagHasChildren(info.hash));
  const isMainBranch = isOriginMain(info);
  return (
    <Commit
      commit={info}
      key={info.hash}
      previewType={info.previewType}
      hasChildren={hasChildren}
      isOriginMain={isMainBranch}
    />
  );
}

const dagHasParents = atomFamilyWeak((key: string) => {
  return atom(get => {
    const dag = get(dagWithPreviews);
    return dag.parents(key).size > 0;
  });
});

const dagIsDraftStackRoot = atomFamilyWeak((key: string) => {
  return atom(get => {
    const dag = get(dagWithPreviews);
    return dag.draft(dag.parents(key)).size === 0;
  });
});

function MaybeFetchingAdditionalCommitsRow({hash}: {hash: Hash}) {
  const hasParents = useAtomValue(dagHasParents(hash));
  return hasParents ? null : <FetchingAdditionalCommitsRow />;
}

function MaybeStackActions({hash}: {hash: Hash}) {
  const isDraftStackRoot = useAtomValue(dagIsDraftStackRoot(hash));
  return isDraftStackRoot ? <StackActions hash={hash} /> : null;
}

function HighlightedGlyph({info}: {info: DagCommitInfo}) {
  const highlighted = useAtomValue(isHighlightedCommit(info.hash));

  const highlightCircle = highlighted ? (
    <circle cx={0} cy={0} r={8} fill="transparent" stroke="var(--focus-border)" strokeWidth={4} />
  ) : null;

  return (
    <>
      {highlightCircle}
      <RegularGlyph info={info} />
    </>
  );
}

/**
 * Scroll the middle column to show a commit at the top.
 * Uses native scrollIntoView with CSS scroll-margin-top for padding.
 * Waits for element to exist in DOM if not immediately available.
 */
export function scrollToCommit(hash: string): void {
  const shortHash = hash.slice(0, 8);

  const tryScroll = (attempt: number) => {
    const element = document.getElementById(`commit-${hash}`);

    if (element) {
      console.log(`[scroll] ${shortHash} found on attempt ${attempt}, scrolling`);
      element.scrollIntoView({behavior: 'smooth', block: 'start'});
      return;
    }

    // Element not found - wait for React to render it (up to 5s)
    if (attempt < 100) {
      requestAnimationFrame(() => {
        setTimeout(() => tryScroll(attempt + 1), 50);
      });
    } else {
      console.log(`[scroll] ${shortHash} NOT found after ${attempt} attempts`);
    }
  };

  console.log(`[scroll] ${shortHash} starting`);
  // Wait for next frame before starting to poll
  requestAnimationFrame(() => tryScroll(0));
}

/**
 * Hook to scroll the commit tree to show the selected commit at the top.
 */
function useScrollToSelectedCommit() {
  const selected = useAtomValue(selectedCommits);

  useEffect(() => {
    if (selected.size !== 1) {
      return;
    }
    const hash = Array.from(selected)[0];
    scrollToCommit(hash);
  }, [selected]);
}

/**
 * Skeleton for the top bar during loading.
 * Matches the structure: Pull button, folder dropdown, icon buttons.
 */
function TopBarSkeleton() {
  return (
    <div className="top-bar-skeleton">
      <div className="top-bar-skeleton-left">
        <div className="skeleton-box skeleton-button-wide" />
        <div className="skeleton-box skeleton-button-medium" />
        <div className="skeleton-box skeleton-icon-btn" />
        <div className="skeleton-box skeleton-icon-btn" />
        <div className="skeleton-box skeleton-icon-btn" />
      </div>
      <div className="top-bar-skeleton-right">
        <div className="skeleton-box skeleton-icon-btn" />
        <div className="skeleton-box skeleton-icon-btn" />
        <div className="skeleton-box skeleton-icon-btn" />
        <div className="skeleton-box skeleton-icon-btn" />
        <div className="skeleton-box skeleton-icon-btn" />
      </div>
    </div>
  );
}

/**
 * Loading view with skeleton top bar and spinner for commit tree.
 */
function CommitTreeLoading() {
  return (
    <div className="commit-tree-loading">
      <TopBarSkeleton />
      <div className="commit-tree-loading-spinner">
        <div className="loading-spinner" />
        <span className="loading-text">Loading commits...</span>
      </div>
    </div>
  );
}

export function CommitTreeList() {
  // Make sure we trigger subscription to changes to uncommitted changes *before* we have a tree to render,
  // so we don't miss the first returned uncommitted changes message.
  // TODO: This is a little ugly, is there a better way to tell recoil to start the subscription immediately?
  // Or should we queue/cache messages?
  useAtomValue(latestUncommittedChangesData);
  useMarkOperationsCompleted();

  useArrowKeysToChangeSelection();
  useBackspaceToHideSelected();
  useShortcutToRebaseSelected();
  useScrollToSelectedCommit();

  const isNarrow = useAtomValue(isNarrowCommitTree);

  const {trees} = useAtomValue(treeWithPreviews);
  const fetchError = useAtomValue(commitFetchError);
  return fetchError == null && trees.length === 0 ? (
    <CommitTreeLoading />
  ) : (
    <>
      {fetchError ? <CommitFetchError error={fetchError} /> : null}
      <DagCommitList isNarrow={isNarrow} />
      <MaybeEditStackModal />
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
          <Button
            onClick={() => {
              runOperation(new CreateEmptyInitialCommitOperation());
            }}>
            <T>Create empty initial commit</T>
          </Button>,
        ]}
      />
    );
  }
  return <ErrorNotice title={t('Failed to fetch commits')} error={error} />;
}
