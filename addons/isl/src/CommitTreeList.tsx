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
import {Icon} from 'isl-components/Icon';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {ErrorShortMessages} from 'isl-server/src/constants';
import {atom, useAtom, useAtomValue, useSetAtom} from 'jotai';
import {useEffect, useRef} from 'react';
import {Commit, InlineProgressSpan} from './Commit';
import {commitTreeSearchFilter} from './CommitTreeSearchFilter';
import {Center, LargeSpinner} from './ComponentUtils';
import {EmptyState} from './EmptyState';
import {FetchingAdditionalCommitsRow} from './FetchAdditionalCommitsButton';
import {isHighlightedCommit} from './HighlightedCommits';
import {RegularGlyph, RenderDag, YouAreHereGlyph} from './RenderDag';
import {StackActions} from './StackActions';
import {latestCommitMessageTitle} from './codeReview/CodeReviewInfo';
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
import {commitFetchError, latestUncommittedChangesData} from './serverAPIState';
import {MaybeEditStackModal} from './stackEdit/ui/EditStackModal';

import './CommitTreeList.css';
import {tracker} from './analytics';
import {focusMode} from './atoms/FocusModeState';

type DagCommitListProps = {
  isNarrow: boolean;
};

const YOU_ARE_HERE_ANCHOR_ID = 'isl-you-are-here-anchor';

/**
 * Where the "You are here" row is relative to the scroll viewport: visible, or
 * scrolled off the top ('above') / bottom ('below'). Drives the floating button.
 */
type YouAreHerePosition = 'visible' | 'above' | 'below';
const youAreHerePosition = atom<YouAreHerePosition>('visible');

/**
 * Scroll the commit graph so the "You are here" row is visible.
 * Returns false if the anchor is not in the DOM yet (nothing scrolled).
 */
export function scrollToYouAreHere(behavior: ScrollBehavior = 'smooth'): boolean {
  // ponytail: getElementById avoids threading a ref from the DAG row up to the TopBar button.
  const anchor = document.getElementById(YOU_ARE_HERE_ANCHOR_ID);
  // The anchor is a zero-height span trailing the badge; scroll the container so the
  // badge above it isn't clipped at the top edge.
  const container = anchor?.parentElement;
  if (container == null) {
    return false;
  }
  container.scrollIntoView({behavior, block: 'start'});
  return true;
}

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

/** Opt-in because auto-scrolling on open can be intrusive. */
export const scrollToYouAreHereOnOpen = localStorageBackedAtom<boolean>(
  'isl.scroll-to-you-are-here-on-open',
  false,
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

  const searchFilter = get(commitTreeSearchFilter).trim().toLowerCase();
  if (searchFilter.length > 0) {
    const matchesSearch = (commit: DagCommitInfo) => {
      if (commit.isYouAreHere) {
        return true;
      }
      const renderedTitle = get(latestCommitMessageTitle(commit.hash));
      const searchable = [
        renderedTitle,
        commit.diffId ?? '',
        ...commit.bookmarks,
        ...commit.remoteBookmarks,
      ];
      return searchable.some(s => s.toLowerCase().includes(searchFilter));
    };
    return dag.filter(matchesSearch, subset.union(selection));
  }

  return subset.union(selection);
});

function DagCommitList(props: DagCommitListProps) {
  const {isNarrow} = props;

  const dag = useAtomValue(dagWithYouAreHere);
  const subset = useAtomValue(renderSubsetUnionSelection);
  const searchFilter = useAtomValue(commitTreeSearchFilter);
  const setSearchFilter = useSetAtom(commitTreeSearchFilter);

  // Check if filter is active and no commits (excluding "You are here") match
  const filter = searchFilter.trim().toLowerCase();
  let hasNoResults = false;
  if (filter.length > 0) {
    let hasMatchingCommit = false;
    for (const hash of subset) {
      const commit = dag.get(hash);
      if (commit && !commit.isYouAreHere) {
        hasMatchingCommit = true;
        break;
      }
    }
    hasNoResults = !hasMatchingCommit;
  }

  if (hasNoResults) {
    return (
      <EmptyState>
        <T>No commits match your filter</T>
        <Button onClick={() => setSearchFilter('')}>
          <T>Clear filter</T>
        </Button>
      </EmptyState>
    );
  }

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
    />
  );
}

function FocusModeIndicator() {
  const [focused, setFocused] = useAtom(focusMode);
  if (!focused) {
    return null;
  }
  return (
    <Button
      onClick={() => {
        tracker.track('SetFocusMode', {extras: {focus: false}});
        setFocused(false);
      }}
      icon>
      <Icon icon="screen-normal" />
      <T>Focus mode is on. Disable to see additional commits</T>
    </Button>
  );
}

function renderCommit(info: DagCommitInfo) {
  return <DagCommitBody info={info} />;
}

function renderCommitExtras(info: DagCommitInfo, row: ExtendedGraphRow) {
  return <CommitExtras info={info} row={row} />;
}

function CommitExtras({info, row}: {info: DagCommitInfo; row: ExtendedGraphRow}) {
  const focused = useAtomValue(focusMode);
  if (row.termLine != null && (info.parents.length > 0 || (info.ancestors?.size ?? 0) > 0)) {
    // Root (no parents) in the displayed DAG, but not root in the full DAG.
    return focused ? (
      <FocusModeIndicator />
    ) : (
      <MaybeFetchingAdditionalCommitsRow hash={info.hash} />
    );
  } else if (info.phase === 'draft') {
    // Draft but parents are not drafts. Likely a stack root. Show stack buttons.
    return <MaybeStackActions hash={info.hash} />;
  }
  return null;
}

function renderGlyph(info: DagCommitInfo): RenderGlyphResult {
  if (info.isYouAreHere) {
    return ['replace-tile', <YouAreHereGlyphWithProgress key="glyph" info={info} />];
  } else {
    return ['inside-tile', <HighlightedGlyph key="glyph" info={info} />];
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

function YouAreHereGlyphWithProgress({info}: {info: DagCommitInfo}) {
  const inlineProgress = useAtomValue(inlineProgressByHash(info.hash));
  const setPosition = useSetAtom(youAreHerePosition);
  const anchorRef = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    // Observe the badge container (the anchor's parent has real height) to drive the
    // floating "scroll to here" button, which shows when the row is off-screen and
    // points toward wherever it scrolled off.
    const el = anchorRef.current?.parentElement;
    if (el == null) {
      return;
    }
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setPosition('visible');
          return;
        }
        const bounds = entry.rootBounds;
        const above = bounds != null && entry.boundingClientRect.top < bounds.top;
        setPosition(above ? 'above' : 'below');
      },
      {root: el.closest('.main-content-area'), threshold: 0},
    );
    observer.observe(el);
    return () => {
      observer.disconnect();
      // No "You are here" row rendered -> nothing to scroll to, so hide the button.
      setPosition('visible');
    };
  }, [setPosition]);
  return (
    <YouAreHereGlyph info={info}>
      <span id={YOU_ARE_HERE_ANCHOR_ID} ref={anchorRef} />
      {inlineProgress && <InlineProgressSpan message={inlineProgress} />}
    </YouAreHereGlyph>
  );
}

// Sticky (not fixed) so it centers over the commit-tree column (`.main-content-area`)
// rather than the whole panel — the commit-info sidebar is a separate drawer outside it.
// Rendered in the 'above' slot (first child, sticks to top) or 'below' slot (last child,
// sticks to bottom) so it points toward wherever the current commit scrolled off.
function ScrollToCurrentCommitButton({slot}: {slot: 'above' | 'below'}) {
  const position = useAtomValue(youAreHerePosition);
  if (position !== slot) {
    return null;
  }
  const atTop = slot === 'above';
  return (
    <div className={'scroll-to-current-commit ' + (atTop ? 'floating-top' : 'floating-bottom')}>
      <Tooltip
        delayMs={DOCUMENTATION_DELAY}
        placement={atTop ? 'bottom' : 'top'}
        title={<T>Scroll to your current commit ("You are here")</T>}>
        <Button
          primary
          className="scroll-to-current-commit-pill"
          onClick={() => scrollToYouAreHere('smooth')}
          data-testid="scroll-to-current-commit-button">
          <Icon icon={atTop ? 'arrow-up' : 'arrow-down'} />
          <T>Scroll to current commit</T>
        </Button>
      </Tooltip>
    </div>
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
  return (
    <Commit
      commit={info}
      key={info.hash}
      previewType={info.previewType}
      hasChildren={hasChildren}
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

  const isNarrow = useAtomValue(isNarrowCommitTree);

  const {trees} = useAtomValue(treeWithPreviews);
  const fetchError = useAtomValue(commitFetchError);
  const shouldScrollToYouAreHereOnOpen = useAtomValue(scrollToYouAreHereOnOpen);

  const hasAutoScrolled = useRef(false);
  useEffect(() => {
    if (!shouldScrollToYouAreHereOnOpen || hasAutoScrolled.current || trees.length === 0) {
      return;
    }
    hasAutoScrolled.current = scrollToYouAreHere('auto');
  }, [trees, shouldScrollToYouAreHereOnOpen]);

  return fetchError == null && trees.length === 0 ? (
    <Center>
      <LargeSpinner />
    </Center>
  ) : (
    <>
      {fetchError ? <CommitFetchError error={fetchError} /> : null}
      <ScrollToCurrentCommitButton slot="above" />
      <DagCommitList isNarrow={isNarrow} />
      <ScrollToCurrentCommitButton slot="below" />
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
            key="create-initial-commit"
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
