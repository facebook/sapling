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
import {Commit, InlineProgressSpan} from './Commit';
import {Center, LargeSpinner} from './ComponentUtils';
import {FetchingAdditionalCommitsRow} from './FetchAdditionalCommitsButton';
import {isHighlightedCommit} from './HighlightedCommits';
import {RegularGlyph, RenderDag, YouAreHereGlyph} from './RenderDag';
import {StackActions} from './StackActions';
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
  return fetchError == null && trees.length === 0 ? (
    <Center>
      <LargeSpinner />
    </Center>
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
