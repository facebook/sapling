/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RenderGlyphResult} from './RenderDag';
import type {Dag, DagCommitInfo} from './dag/dag';
import type {ExtendedGraphRow} from './dag/render';
import type {HashSet} from './dag/set';
import type {Hash} from './types';

import serverAPI from './ClientToServerAPI';
import {Commit} from './Commit';
import {Center, LargeSpinner} from './ComponentUtils';
import {ErrorNotice} from './ErrorNotice';
import {isHighlightedCommit} from './HighlightedCommits';
import {RegularGlyph, RenderDag, YouAreHereGlyph} from './RenderDag';
import {StackActions} from './StackActions';
import {Tooltip, DOCUMENTATION_DELAY} from './Tooltip';
import {YOU_ARE_HERE_VIRTUAL_COMMIT} from './dag/virtualCommit';
import {T, t} from './i18n';
import {atomFamilyWeak} from './jotaiUtils';
import {CreateEmptyInitialCommitOperation} from './operations/CreateEmptyInitialCommitOperation';
import {dagWithPreviews, treeWithPreviews, useMarkOperationsCompleted} from './previews';
import {isNarrowCommitTree} from './responsive';
import {useArrowKeysToChangeSelection, useBackspaceToHideSelected} from './selection';
import {
  commitFetchError,
  commitsShownRange,
  isFetchingAdditionalCommits,
  latestUncommittedChangesData,
  useRunOperation,
} from './serverAPIState';
import {MaybeEditStackModal} from './stackEdit/ui/EditStackModal';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {ErrorShortMessages} from 'isl-server/src/constants';
import {atom, useAtomValue} from 'jotai';
import {Icon} from 'shared/Icon';

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

function DagCommitList(props: DagCommitListProps) {
  const {isNarrow} = props;

  const dag = useAtomValue(dagWithYouAreHere);
  const subset = getRenderSubset(dag);

  return (
    <RenderDag
      dag={dag}
      subset={subset}
      className={'commit-tree-root ' + (isNarrow ? ' commit-tree-narrow' : '')}
      data-testid="commit-tree-root"
      renderCommit={renderCommit}
      renderCommitExtras={renderCommitExtras}
      renderGlyph={renderGlyph}
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
    return ['replace-tile', <YouAreHereGlyph info={info} />];
  } else {
    return ['inside-tile', <HighlightedGlyph info={info} />];
  }
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

  const hilightCircle = highlighted ? (
    <circle cx={0} cy={0} r={8} fill="transparent" stroke="var(--focus-border)" strokeWidth={4} />
  ) : null;

  return (
    <>
      {hilightCircle}
      <RegularGlyph info={info} />
    </>
  );
}

function getRenderSubset(dag: Dag): HashSet {
  return dag.subsetForRendering();
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

function FetchingAdditionalCommitsRow() {
  return (
    <div className="fetch-additional-commits-row">
      <FetchingAdditionalCommitsButton />
      <FetchingAdditionalCommitsIndicator />
    </div>
  );
}

function FetchingAdditionalCommitsIndicator() {
  const isFetching = useAtomValue(isFetchingAdditionalCommits);
  return isFetching ? <Icon icon="loading" /> : null;
}

function FetchingAdditionalCommitsButton() {
  const shownRange = useAtomValue(commitsShownRange);
  const isFetching = useAtomValue(isFetchingAdditionalCommits);
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
