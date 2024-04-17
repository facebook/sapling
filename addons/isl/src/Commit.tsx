/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {DagCommitInfo} from './dag/dag';
import type {CommitInfo, SuccessorInfo} from './types';
import type {ContextMenuItem} from 'shared/ContextMenu';

import {Bookmarks} from './Bookmark';
import {hasUnsavedEditedCommitMessage} from './CommitInfoView/CommitInfoState';
import {currentComparisonMode} from './ComparisonView/atoms';
import {Row} from './ComponentUtils';
import {DragToRebase} from './DragToRebase';
import {EducationInfoTip} from './Education';
import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {SubmitSelectionButton} from './SubmitSelectionButton';
import {Subtle} from './Subtle';
import {latestSuccessorUnlessExplicitlyObsolete} from './SuccessionTracker';
import {getSuggestedRebaseOperation, suggestedRebaseDestinations} from './SuggestedRebase';
import {Tooltip} from './Tooltip';
import {UncommitButton} from './UncommitButton';
import {UncommittedChanges} from './UncommittedChanges';
import {tracker} from './analytics';
import {clipboardLinkHtml} from './clipboard';
import {
  codeReviewProvider,
  diffSummary,
  latestCommitMessageTitle,
} from './codeReview/CodeReviewInfo';
import {DiffFollower, DiffInfo} from './codeReview/DiffBadge';
import {SyncStatus, syncStatusAtom} from './codeReview/syncStatus';
import {FoldButton, useRunFoldPreview} from './fold';
import {t, T} from './i18n';
import {IconStack} from './icons/IconStack';
import {readAtom, writeAtom} from './jotaiUtils';
import {getAmendToOperation, isAmendToAllowedForCommit} from './operationUtils';
import {GotoOperation} from './operations/GotoOperation';
import {HideOperation} from './operations/HideOperation';
import {
  operationBeingPreviewed,
  useRunOperation,
  useRunPreviewedOperation,
  inlineProgressByHash,
} from './operationsState';
import {CommitPreview, dagWithPreviews, uncommittedChangesWithPreviews} from './previews';
import {RelativeDate} from './relativeDate';
import {isNarrowCommitTree} from './responsive';
import {selectedCommits, useCommitCallbacks} from './selection';
import {useConfirmUnsavedEditsBeforeSplit} from './stackEdit/ui/ConfirmUnsavedEditsBeforeSplit';
import {SplitButton} from './stackEdit/ui/SplitButton';
import {editingStackIntentionHashes} from './stackEdit/ui/stackEditState';
import {useShowToast} from './toast';
import {succeedableRevset} from './types';
import {short} from './utils';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useAtomValue, useSetAtom} from 'jotai';
import {useAtomCallback} from 'jotai/utils';
import React, {memo} from 'react';
import {ComparisonType} from 'shared/Comparison';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {useAutofocusRef} from 'shared/hooks';
import {notEmpty} from 'shared/utils';

/**
 * Some preview types should not allow actions on top of them
 * For example, you can't click goto on the preview of dragging a rebase,
 * but you can goto on the optimistic form of a running rebase.
 */
function previewPreventsActions(preview?: CommitPreview): boolean {
  switch (preview) {
    case CommitPreview.REBASE_OLD:
    case CommitPreview.REBASE_DESCENDANT:
    case CommitPreview.REBASE_ROOT:
    case CommitPreview.HIDDEN_ROOT:
    case CommitPreview.HIDDEN_DESCENDANT:
    case CommitPreview.FOLD:
    case CommitPreview.FOLD_PREVIEW:
    case CommitPreview.NON_ACTIONABLE_COMMIT:
      return true;
  }
  return false;
}

export const Commit = memo(
  ({
    commit,
    previewType,
    hasChildren,
  }: {
    commit: DagCommitInfo | CommitInfo;
    previewType?: CommitPreview;
    hasChildren: boolean;
  }) => {
    const isPublic = commit.phase === 'public';
    const isObsoleted = commit.successorInfo != null;

    const handlePreviewedOperation = useRunPreviewedOperation();
    const runOperation = useRunOperation();
    const setEditStackIntentionHashes = useSetAtom(editingStackIntentionHashes);

    const inlineProgress = useAtomValue(inlineProgressByHash(commit.hash));

    const {isSelected, onDoubleClickToShowDrawer} = useCommitCallbacks(commit);
    const actionsPrevented = previewPreventsActions(previewType);

    const isNarrow = useAtomValue(isNarrowCommitTree);

    const title = useAtomValue(latestCommitMessageTitle(commit.hash));

    const toast = useShowToast();

    const clipboardCopy = (text: string, url?: string) =>
      toast.copyAndShowToast(text, url == null ? undefined : clipboardLinkHtml(text, url));

    const viewChangesCallback = useAtomCallback((_get, set) => {
      set(currentComparisonMode, {
        comparison: {type: ComparisonType.Committed, hash: commit.hash},
        visible: true,
      });
    });

    const confirmUnsavedEditsBeforeSplit = useConfirmUnsavedEditsBeforeSplit();
    async function handleSplit() {
      if (!(await confirmUnsavedEditsBeforeSplit([commit], 'split'))) {
        return;
      }
      setEditStackIntentionHashes(['split', new Set([commit.hash])]);
      tracker.track('SplitOpenFromCommitContextMenu');
    }

    const makeContextMenuOptions = () => {
      const hasUncommittedChanges = (readAtom(uncommittedChangesWithPreviews).length ?? 0) > 0;
      const syncStatus = readAtom(syncStatusAtom)?.get(commit.hash);

      const items: Array<ContextMenuItem> = [
        {
          label: <T replace={{$hash: short(commit?.hash)}}>Copy Commit Hash "$hash"</T>,
          onClick: () => clipboardCopy(commit.hash),
        },
      ];
      if (!isPublic && commit.diffId != null) {
        items.push({
          label: <T replace={{$number: commit.diffId}}>Copy Diff Number "$number"</T>,
          onClick: () => {
            const info = readAtom(diffSummary(commit.diffId));
            const url = info?.value?.url;
            clipboardCopy(commit.diffId ?? '', url);
          },
        });
      }
      if (!isPublic) {
        items.push({
          label: <T>View Changes in Commit</T>,
          onClick: viewChangesCallback,
        });
      }
      if (
        !isPublic &&
        (syncStatus === SyncStatus.LocalIsNewer || syncStatus === SyncStatus.RemoteIsNewer)
      ) {
        const provider = readAtom(codeReviewProvider);
        if (provider?.supportsComparingSinceLastSubmit) {
          items.push({
            label: <T replace={{$provider: provider?.label ?? 'remote'}}>Compare with $provider</T>,
            onClick: () => {
              writeAtom(currentComparisonMode, {
                comparison: {type: ComparisonType.SinceLastCodeReviewSubmit, hash: commit.hash},
                visible: true,
              });
            },
          });
        }
      }
      if (!isPublic && !actionsPrevented) {
        const suggestedRebases = readAtom(suggestedRebaseDestinations);
        items.push({
          label: 'Rebase onto',
          type: 'submenu',
          children:
            suggestedRebases?.map(([dest, name]) => ({
              label: <T>{name}</T>,
              onClick: () => {
                const operation = getSuggestedRebaseOperation(
                  dest,
                  latestSuccessorUnlessExplicitlyObsolete(commit),
                );
                runOperation(operation);
              },
            })) ?? [],
        });
        if (isAmendToAllowedForCommit(commit)) {
          items.push({
            label: <T>Amend changes to here</T>,
            onClick: () => runOperation(getAmendToOperation(commit)),
          });
        }
        if (!isObsoleted) {
          items.push({
            label: hasUncommittedChanges ? (
              <span className="context-menu-disabled-option">
                <T>Split... </T>
                <Subtle>
                  <T>(disabled due to uncommitted changes)</T>
                </Subtle>
              </span>
            ) : (
              <T>Split...</T>
            ),
            onClick: hasUncommittedChanges ? () => null : handleSplit,
          });
        }
        items.push({
          label: hasChildren ? <T>Hide Commit and Descendants</T> : <T>Hide Commit</T>,
          onClick: () =>
            writeAtom(
              operationBeingPreviewed,
              new HideOperation(latestSuccessorUnlessExplicitlyObsolete(commit)),
            ),
        });
      }
      return items;
    };

    const contextMenu = useContextMenu(makeContextMenuOptions);

    const commitActions = [];

    if (previewType === CommitPreview.REBASE_ROOT) {
      commitActions.push(
        <React.Fragment key="rebase">
          <VSCodeButton
            appearance="secondary"
            onClick={() => handlePreviewedOperation(/* cancel */ true)}>
            <T>Cancel</T>
          </VSCodeButton>
          <VSCodeButton
            appearance="primary"
            onClick={() => {
              handlePreviewedOperation(/* cancel */ false);

              const dag = readAtom(dagWithPreviews);
              const onto = dag.get(commit.parents[0]);
              if (onto) {
                tracker.track('ConfirmDragAndDropRebase', {
                  extras: {
                    remoteBookmarks: onto.remoteBookmarks,
                    locations: onto.stableCommitMetadata?.map(s => s.value),
                  },
                });
              }
            }}>
            <T>Run Rebase</T>
          </VSCodeButton>
        </React.Fragment>,
      );
    } else if (previewType === CommitPreview.HIDDEN_ROOT) {
      commitActions.push(
        <React.Fragment key="hide">
          <VSCodeButton
            appearance="secondary"
            onClick={() => handlePreviewedOperation(/* cancel */ true)}>
            <T>Cancel</T>
          </VSCodeButton>
          <ConfirmHideButton onClick={() => handlePreviewedOperation(/* cancel */ false)} />
        </React.Fragment>,
      );
    } else if (previewType === CommitPreview.FOLD_PREVIEW) {
      commitActions.push(<ConfirmCombineButtons key="fold" />);
    }

    if (!isPublic && !actionsPrevented && isSelected) {
      commitActions.push(
        <SubmitSelectionButton key="submit-selection-btn" commit={commit} />,
        <FoldButton key="fold-button" commit={commit} />,
      );
    }

    if (!actionsPrevented && !commit.isDot) {
      commitActions.push(
        <span className="goto-button" key="goto-button">
          <Tooltip
            title={t(
              'Update files in the working copy to match this commit. Mark this commit as the "current commit".',
            )}
            delayMs={250}>
            <VSCodeButton
              appearance="secondary"
              aria-label={t('Go to commit "$title"', {replace: {$title: commit.title}})}
              onClick={event => {
                runOperation(
                  new GotoOperation(
                    // If the commit has a remote bookmark, use that instead of the hash. This is easier to read in the command history
                    // and works better with optimistic state
                    commit.remoteBookmarks.length > 0
                      ? succeedableRevset(commit.remoteBookmarks[0])
                      : latestSuccessorUnlessExplicitlyObsolete(commit),
                  ),
                );
                event.stopPropagation(); // don't toggle selection by letting click propagate onto selection target.
                // Instead, ensure we remove the selection, so we view the new head commit by default
                // (since the head commit is the default thing shown in the sidebar)
                writeAtom(selectedCommits, new Set());
              }}>
              <T>Goto</T> <Icon icon="newline" />
            </VSCodeButton>
          </Tooltip>
        </span>,
      );
    }

    if (!isPublic && !actionsPrevented && commit.isDot) {
      commitActions.push(<UncommitButton key="uncommit" />);
    }
    if (!isPublic && !actionsPrevented && commit.isDot && !isObsoleted) {
      commitActions.push(<SplitButton key="split" commit={commit} />);
    }

    if (!isPublic && !actionsPrevented) {
      commitActions.push(
        <OpenCommitInfoButton
          key="open-sidebar"
          revealCommit={onDoubleClickToShowDrawer}
          commit={commit}
        />,
      );
    }

    if ((commit as DagCommitInfo).isYouAreHere) {
      return (
        <div className="head-commit-info">
          <UncommittedChanges place="main" />
        </div>
      );
    }

    return (
      <div
        className={
          'commit' +
          (commit.isDot ? ' head-commit' : '') +
          (commit.successorInfo != null ? ' obsolete' : '')
        }
        onContextMenu={contextMenu}
        data-testid={`commit-${commit.hash}`}>
        <div className={'commit-rows'} data-testid={isSelected ? 'selected-commit' : undefined}>
          <DragToRebase
            className={
              'commit-details' + (previewType != null ? ` commit-preview-${previewType}` : '')
            }
            commit={commit}
            previewType={previewType}>
            {isPublic ? null : (
              <span className="commit-title">
                <span>{title}</span>
                <CommitDate date={commit.date} />
              </span>
            )}
            <UnsavedEditedMessageIndicator commit={commit} />
            <Bookmarks bookmarks={commit.bookmarks} kind="local" />
            <Bookmarks bookmarks={commit.remoteBookmarks} kind="remote" />
            {commit?.stableCommitMetadata != null ? (
              <Bookmarks bookmarks={commit.stableCommitMetadata} kind="stable" />
            ) : null}
            {isPublic ? <CommitDate date={commit.date} /> : null}
            {isNarrow ? commitActions : null}
          </DragToRebase>
          <DivIfChildren className="commit-second-row">
            {commit.diffId && !isPublic ? (
              <DiffInfo commit={commit} hideActions={actionsPrevented || inlineProgress != null} />
            ) : null}
            {commit.successorInfo != null ? (
              <SuccessorInfoToDisplay successorInfo={commit.successorInfo} />
            ) : null}
            {inlineProgress && <InlineProgressSpan message={inlineProgress} />}
            {commit.isFollower ? <DiffFollower commit={commit} /> : null}
          </DivIfChildren>
          {!isNarrow ? commitActions : null}
        </div>
      </div>
    );
  },
  (prevProps, nextProps) => {
    const prevCommit = prevProps.commit;
    const nextCommit = nextProps.commit;
    const commitEqual =
      'equals' in nextCommit ? nextCommit.equals(prevCommit) : nextCommit === prevCommit;
    return (
      commitEqual &&
      nextProps.previewType === prevProps.previewType &&
      nextProps.hasChildren === prevProps.hasChildren
    );
  },
);

export function InlineProgressSpan(props: {message: string}) {
  return (
    <span className="commit-inline-operation-progress">
      <Icon icon="loading" /> <T>{props.message}</T>
    </span>
  );
}

function OpenCommitInfoButton({
  commit,
  revealCommit,
}: {
  commit: CommitInfo;
  revealCommit: () => unknown;
}) {
  return (
    <Tooltip title={t("Open commit's details in sidebar")} delayMs={250}>
      <VSCodeButton
        appearance="icon"
        onClick={e => {
          revealCommit();
          e.stopPropagation();
          e.preventDefault();
        }}
        className="open-commit-info-button"
        aria-label={t('Open commit "$title"', {replace: {$title: commit.title}})}
        data-testid="open-commit-info-button">
        <Icon icon="chevron-right" />
      </VSCodeButton>
    </Tooltip>
  );
}

function ConfirmHideButton({onClick}: {onClick: () => unknown}) {
  const ref = useAutofocusRef() as React.MutableRefObject<null>;
  return (
    <VSCodeButton ref={ref} appearance="primary" onClick={onClick}>
      <T>Hide</T>
    </VSCodeButton>
  );
}

function ConfirmCombineButtons() {
  const ref = useAutofocusRef() as React.MutableRefObject<null>;
  const [cancel, run] = useRunFoldPreview();

  return (
    <>
      <VSCodeButton appearance="secondary" onClick={cancel}>
        <T>Cancel</T>
      </VSCodeButton>
      <VSCodeButton ref={ref} appearance="primary" onClick={run}>
        <T>Run Combine</T>
      </VSCodeButton>
    </>
  );
}

function CommitDate({date}: {date: Date}) {
  return (
    <span className="commit-date" title={date.toLocaleString()}>
      <RelativeDate date={date} useShortVariant />
    </span>
  );
}

function DivIfChildren({
  children,
  ...props
}: React.DetailedHTMLProps<React.HTMLAttributes<HTMLDivElement>, HTMLDivElement>) {
  if (!children || (Array.isArray(children) && children.filter(notEmpty).length === 0)) {
    return null;
  }
  return <div {...props}>{children}</div>;
}

function UnsavedEditedMessageIndicator({commit}: {commit: CommitInfo}) {
  const isEdted = useAtomValue(hasUnsavedEditedCommitMessage(commit.hash));
  if (!isEdted) {
    return null;
  }
  return (
    <div className="unsaved-message-indicator" data-testid="unsaved-message-indicator">
      <Tooltip title={t('This commit has unsaved changes to its message')}>
        <IconStack>
          <Icon icon="output" />
          <Icon icon="circle-large-filled" />
        </IconStack>
      </Tooltip>
    </div>
  );
}

export function SuccessorInfoToDisplay({successorInfo}: {successorInfo: SuccessorInfo}) {
  const successorType = successorInfo.type;
  const inner: JSX.Element = {
    pushrebase: <T>Landed as a newer commit</T>,
    land: <T>Landed as a newer commit</T>,
    amend: <T>Amended as a newer commit</T>,
    rebase: <T>Rebased as a newer commit</T>,
    split: <T>Split as a newer commit</T>,
    fold: <T>Folded as a newer commit</T>,
    histedit: <T>Histedited as a newer commit</T>,
  }[successorType] ?? <T>Rewritten as a newer commit</T>;
  const isSuccessorPublic = successorType === 'land' || successorType === 'pushrebase';
  return (
    <Row style={{gap: 'var(--halfpad)'}}>
      <HighlightCommitsWhileHovering toHighlight={[successorInfo.hash]}>
        {inner}
      </HighlightCommitsWhileHovering>
      <EducationInfoTip>
        <ObsoleteTip isSuccessorPublic={isSuccessorPublic} />
      </EducationInfoTip>
    </Row>
  );
}

function ObsoleteTipInner(props: {isSuccessorPublic?: boolean}) {
  const tips: string[] = props.isSuccessorPublic
    ? [
        t('Avoid editing (e.g., amend, rebase) this obsoleted commit. It cannot be landed again.'),
        t(
          'The new commit was landed in a public branch and became immutable. It cannot be edited or hidden.',
        ),
        t('If you want to make changes, create a new commit.'),
      ]
    : [
        t(
          'Avoid editing (e.g., amend, rebase) this obsoleted commit. You should use the new commit instead.',
        ),
        t(
          'If you do edit, there will be multiple new versions. They look like duplications and there is no easy way to de-duplicate (e.g. merge all edits back into one commit).',
        ),
        t(
          'To revert to this obsoleted commit, simply hide the new one. It will remove the "obsoleted" status.',
        ),
      ];

  return (
    <div style={{maxWidth: '60vw'}}>
      <T>This commit is "obsoleted" because a newer version exists.</T>
      <ul>
        {tips.map((tip, i) => (
          <li key={i}>{tip}</li>
        ))}
      </ul>
    </div>
  );
}

const ObsoleteTip = React.memo(ObsoleteTipInner);
