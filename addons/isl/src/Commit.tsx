/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {ContextMenuItem} from 'shared/ContextMenu';
import type {UICodeReviewProvider} from './codeReview/UICodeReviewProvider';
import type {DagCommitInfo} from './dag/dag';
import type {CommitInfo, SuccessorInfo} from './types';
import {succeedableRevset, WarningCheckResult} from './types';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtomValue, useSetAtom} from 'jotai';
import React, {memo} from 'react';
import {ComparisonType} from 'shared/Comparison';
import {useContextMenu} from 'shared/ContextMenu';
import {MS_PER_DAY} from 'shared/constants';
import {useAutofocusRef} from 'shared/hooks';
import {notEmpty, nullthrows} from 'shared/utils';
import {spacing} from '../../components/theme/tokens.stylex';
import {AllBookmarksTruncated, Bookmark, Bookmarks, createBookmarkAtCommit} from './Bookmark';
import {openBrowseUrlForHash, supportsBrowseUrlForHash} from './BrowseRepo';
import {hasUnsavedEditedCommitMessage} from './CommitInfoView/CommitInfoState';
import {showComparison} from './ComparisonView/atoms';
import {Row} from './ComponentUtils';
import {DragToRebase} from './DragToRebase';
import {EducationInfoTip} from './Education';
import {HighlightCommitsWhileHovering} from './HighlightedCommits';
import {Internal} from './Internal';
import {SmartActionsMenu} from './SmartActionsMenu';
import {SubmitSelectionButton} from './SubmitSelectionButton';
import {getSuggestedRebaseOperation, suggestedRebaseDestinations} from './SuggestedRebase';
import {UncommitButton} from './UncommitButton';
import {UncommittedChanges} from './UncommittedChanges';
import {tracker} from './analytics';
import {clipboardLinkHtml} from './clipboard';
import {
  branchingDiffInfos,
  codeReviewProvider,
  diffSummary,
  latestCommitMessageTitle,
} from './codeReview/CodeReviewInfo';
import {DiffBadge, DiffFollower, DiffInfo} from './codeReview/DiffBadge';
import {SyncStatus, syncStatusAtom} from './codeReview/syncStatus';
import {FoldButton, useRunFoldPreview} from './fold';
import {findPublicBaseAncestor} from './getCommitTree';
import {t, T} from './i18n';
import {IconStack} from './icons/IconStack';
import {IrrelevantCwdIcon} from './icons/IrrelevantCwdIcon';
import {atomFamilyWeak, localStorageBackedAtom, readAtom, writeAtom} from './jotaiUtils';
import {CONFLICT_SIDE_LABELS} from './mergeConflicts/state';
import {getAmendToOperation, isAmendToAllowedForCommit} from './operationUtils';
import {GotoOperation} from './operations/GotoOperation';
import {HideOperation} from './operations/HideOperation';
import {
  inlineProgressByHash,
  operationBeingPreviewed,
  useRunOperation,
  useRunPreviewedOperation,
} from './operationsState';
import platform from './platform';
import {CommitPreview, dagWithPreviews, uncommittedChangesWithPreviews} from './previews';
import {RelativeDate, relativeDate} from './relativeDate';
import {repoRelativeCwd, useIsIrrelevantToCwd} from './repositoryData';
import {isNarrowCommitTree} from './responsive';
import {selectedCommits, useCommitCallbacks} from './selection';
import {inMergeConflicts, mergeConflicts} from './serverAPIState';
import {useConfirmUnsavedEditsBeforeSplit} from './stackEdit/ui/ConfirmUnsavedEditsBeforeSplit';
import {SplitButton} from './stackEdit/ui/SplitButton';
import {editingStackIntentionHashes} from './stackEdit/ui/stackEditState';
import {latestSuccessorUnlessExplicitlyObsolete} from './successionUtils';
import {copyAndShowToast} from './toast';
import {showModal} from './useModal';
import {short} from './utils';

export const rebaseOffWarmWarningEnabled = localStorageBackedAtom<boolean>(
  'isl.rebase-off-warm-warning-enabled',
  true,
);

export const distantRebaseWarningEnabled = localStorageBackedAtom<boolean>(
  'isl.distant-rebase-warning-enabled',
  true,
);

export const rebaseOntoMasterWarningEnabled = localStorageBackedAtom<boolean>(
  'isl.rebase-onto-master-warning-enabled',
  true,
);

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

const commitLabelForCommit = atomFamilyWeak((hash: string) =>
  atom(get => {
    const conflicts = get(mergeConflicts);
    const {localShort, incomingShort} = CONFLICT_SIDE_LABELS;
    const hashes = conflicts?.hashes;
    if (hash === hashes?.other) {
      return incomingShort;
    } else if (hash === hashes?.local) {
      return localShort;
    }
    return null;
  }),
);

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

    const isIrrelevantToCwd = useIsIrrelevantToCwd(commit);

    const handlePreviewedOperation = useRunPreviewedOperation();
    const runOperation = useRunOperation();
    const setEditStackIntentionHashes = useSetAtom(editingStackIntentionHashes);

    const inlineProgress = useAtomValue(inlineProgressByHash(commit.hash));

    const {isSelected, onDoubleClickToShowDrawer} = useCommitCallbacks(commit);
    const actionsPrevented = previewPreventsActions(previewType);

    const inConflicts = useAtomValue(inMergeConflicts);

    const isNarrow = useAtomValue(isNarrowCommitTree);

    const title = useAtomValue(latestCommitMessageTitle(commit.hash));

    const commitLabel = useAtomValue(commitLabelForCommit(commit.hash));

    const clipboardCopy = (text: string, url?: string) =>
      copyAndShowToast(text, url == null ? undefined : clipboardLinkHtml(text, url));

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
      if (isPublic && readAtom(supportsBrowseUrlForHash)) {
        items.push({
          label: (
            <Row>
              <T>Browse Repo At This Commit</T>
              <Icon icon="link-external" />
            </Row>
          ),
          onClick: () => {
            openBrowseUrlForHash(commit.hash);
          },
        });
      }
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
          onClick: () => showComparison({type: ComparisonType.Committed, hash: commit.hash}),
        });
      }
      if (!isPublic && syncStatus != null && syncStatus !== SyncStatus.InSync) {
        const provider = readAtom(codeReviewProvider);
        if (provider?.supportsComparingSinceLastSubmit) {
          items.push({
            label: <T replace={{$provider: provider?.label ?? 'remote'}}>Compare with $provider</T>,
            onClick: () => {
              showComparison({type: ComparisonType.SinceLastCodeReviewSubmit, hash: commit.hash});
            },
          });
        }
      }
      if (!isPublic && commit.diffId != null) {
        const provider = readAtom(codeReviewProvider);
        const summary = readAtom(diffSummary(commit.diffId));
        if (summary.value) {
          const actions = provider?.getUpdateDiffActions(summary.value);
          if (actions != null && actions.length > 0) {
            items.push({
              label: <T replace={{$number: commit.diffId}}>Update Diff $number</T>,
              type: 'submenu',
              children: actions,
            });
          }
        }
      }
      if (!isPublic && !actionsPrevented && !inConflicts) {
        const suggestedRebases = readAtom(suggestedRebaseDestinations);
        items.push({
          label: 'Rebase onto',
          type: 'submenu',
          children:
            suggestedRebases?.map(([dest, name]) => ({
              label: name,
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
          label: <T>Create Bookmark...</T>,
          onClick: () => {
            createBookmarkAtCommit(commit);
          },
        });
        items.push({
          label: hasChildren ? <T>Hide Commit and Descendants</T> : <T>Hide Commit</T>,
          onClick: () =>
            writeAtom(
              operationBeingPreviewed,
              new HideOperation(latestSuccessorUnlessExplicitlyObsolete(commit)),
            ),
        });
      }
      if (!actionsPrevented && !commit.isDot) {
        items.push({
          label: <T>Goto</T>,
          onClick: async () => {
            await gotoAction(runOperation, commit);
          },
        });
      }
      return items;
    };

    const contextMenu = useContextMenu(makeContextMenuOptions);

    const commitActions = [];

    if (previewType === CommitPreview.REBASE_ROOT) {
      commitActions.push(
        <React.Fragment key="rebase">
          <Button onClick={() => handlePreviewedOperation(/* cancel */ true)}>
            <T>Cancel</T>
          </Button>
          <Button
            primary
            onClick={() => {
              return handleRebaseConfirmation(commit, handlePreviewedOperation);
            }}>
            <T>Run Rebase</T>
          </Button>
        </React.Fragment>,
      );
    } else if (previewType === CommitPreview.HIDDEN_ROOT) {
      commitActions.push(
        <React.Fragment key="hide">
          <Button onClick={() => handlePreviewedOperation(/* cancel */ true)}>
            <T>Cancel</T>
          </Button>
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
            <Button
              aria-label={t('Go to commit "$title"', {replace: {$title: commit.title}})}
              xstyle={styles.gotoButton}
              onClick={async event => {
                event.stopPropagation(); // don't toggle selection by letting click propagate onto selection target.
                await gotoAction(runOperation, commit);
              }}>
              <T>Goto</T>
              <Icon icon="newline" />
            </Button>
          </Tooltip>
        </span>,
      );
    }

    if (!isPublic && !actionsPrevented && commit.isDot && !inConflicts) {
      commitActions.push(<UncommitButton key="uncommit" />);
    }
    if (!isPublic && !actionsPrevented && commit.isDot && !isObsoleted && !inConflicts) {
      commitActions.push(
        <SplitButton icon key="split" trackerEventName="SplitOpenFromHeadCommit" commit={commit} />,
      );
    }

    if (!isPublic && !actionsPrevented) {
      commitActions.push(
        <SmartActionsMenu key="smartActions" commit={commit} />,
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
          (commit.successorInfo != null ? ' obsolete' : '') +
          (isIrrelevantToCwd ? ' irrelevant' : '')
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
            {!isPublic && isIrrelevantToCwd && (
              <Tooltip
                title={
                  <T
                    replace={{
                      $prefix: <pre>{commit.maxCommonPathPrefix}</pre>,
                      $cwd: <pre>{readAtom(repoRelativeCwd)}</pre>,
                    }}>
                    This commit only contains files within: $prefix These are irrelevant to your
                    current working directory: $cwd
                  </T>
                }>
                <IrrelevantCwdIcon />
              </Tooltip>
            )}
            {isPublic ? null : (
              <span className="commit-title">
                {commitLabel && <CommitLabel>{commitLabel}</CommitLabel>}
                <span>{title}</span>
                <CommitDate date={commit.date} />
              </span>
            )}
            <UnsavedEditedMessageIndicator commit={commit} />
            {!isPublic && <BranchingPrs bookmarks={commit.remoteBookmarks} />}
            <AllBookmarksTruncated
              local={commit.bookmarks}
              remote={
                isPublic
                  ? commit.remoteBookmarks
                  : /* draft commits with remote bookmarks are probably branching PRs, rendered above. */ []
              }
              stable={commit?.stableCommitMetadata ?? []}
            />
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

function BranchingPrs({bookmarks}: {bookmarks: ReadonlyArray<string>}) {
  const provider = useAtomValue(codeReviewProvider);
  if (provider == null || !provider.supportBranchingPrs) {
    // If we don't have a provider, just render them as bookmarks so they don't get hidden.
    return <Bookmarks bookmarks={bookmarks} kind="remote" />;
  }
  return bookmarks.map(bookmark => (
    <BranchingPr key={bookmark} provider={provider} bookmark={bookmark} />
  ));
}

function BranchingPr({bookmark, provider}: {bookmark: string; provider: UICodeReviewProvider}) {
  const branchName = nullthrows(provider.branchNameForRemoteBookmark)(bookmark);
  const info = useAtomValue(branchingDiffInfos(branchName));
  return (
    <>
      <Bookmark kind="remote">{bookmark}</Bookmark>
      {info.value == null ? null : (
        <DiffBadge diff={info.value} provider={provider} url={info.value.url} />
      )}
    </>
  );
}

const styles = stylex.create({
  commitLabel: {
    fontVariant: 'all-petite-caps',
    opacity: '0.8',
    fontWeight: 'bold',
    fontSize: '90%',
  },
  gotoButton: {
    gap: spacing.half,
  },
});

function CommitLabel({children}: {children?: ReactNode}) {
  return <div {...stylex.props(styles.commitLabel)}>{children}</div>;
}

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
      <Button
        icon
        onClick={e => {
          revealCommit();
          e.stopPropagation();
          e.preventDefault();
        }}
        className="open-commit-info-button"
        aria-label={t('Open commit "$title"', {replace: {$title: commit.title}})}
        data-testid="open-commit-info-button">
        <Icon icon="chevron-right" />
      </Button>
    </Tooltip>
  );
}

function ConfirmHideButton({onClick}: {onClick: () => unknown}) {
  const ref = useAutofocusRef() as React.MutableRefObject<null>;
  return (
    <Button ref={ref} primary onClick={onClick}>
      <T>Hide</T>
    </Button>
  );
}

function ConfirmCombineButtons() {
  const ref = useAutofocusRef() as React.MutableRefObject<null>;
  const [cancel, run] = useRunFoldPreview();

  return (
    <>
      <Button onClick={cancel}>
        <T>Cancel</T>
      </Button>
      <Button ref={ref} primary onClick={run}>
        <T>Run Combine</T>
      </Button>
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

async function maybeWarnAboutOldDestination(dest: CommitInfo): Promise<WarningCheckResult> {
  const provider = readAtom(codeReviewProvider);
  // Cutoff age is determined by the code review provider since internal repos have different requirements than GitHub-backed repos.
  const MAX_AGE_CUTOFF_MS = provider?.gotoDistanceWarningAgeCutoff ?? 30 * MS_PER_DAY;

  const dag = readAtom(dagWithPreviews);
  const currentBase = findPublicBaseAncestor(dag);
  const destBase = findPublicBaseAncestor(dag, dest.hash);
  if (!currentBase || !destBase) {
    // can't determine if we can show warning
    return Promise.resolve(WarningCheckResult.PASS);
  }

  const ageDiff = currentBase.date.valueOf() - destBase.date.valueOf();
  if (ageDiff < MAX_AGE_CUTOFF_MS) {
    // Either destination base is within time limit or destination base is newer than the current base.
    // No need to warn.
    return Promise.resolve(WarningCheckResult.PASS);
  }

  const confirmed = await platform.confirm(
    t(
      Internal.warnAboutOldGotoReason ??
        'The destination commit is $age older than the current commit. ' +
          "Going here may be slow. It's often faster to rebase the commit to a newer base before going. " +
          'Do you want to `goto` anyway?',
      {
        replace: {
          $age: relativeDate(destBase.date, {reference: currentBase.date, useRelativeForm: true}),
        },
      },
    ),
  );
  return confirmed ? WarningCheckResult.BYPASS : WarningCheckResult.FAIL;
}

async function maybeWarnAboutRebaseOffWarm(dest: CommitInfo): Promise<WarningCheckResult> {
  const isRebaseOffWarmWarningEnabled = readAtom(rebaseOffWarmWarningEnabled);
  if (!isRebaseOffWarmWarningEnabled) {
    return WarningCheckResult.PASS;
  }

  if (dest.stableCommitMetadata == null) {
    return WarningCheckResult.PASS;
  }
  // iterate through stable commit metadata and see if this commit is warmed up commit
  const dag = readAtom(dagWithPreviews);
  const src = findPublicBaseAncestor(dag);

  const destBase = findPublicBaseAncestor(dag, dest.hash);

  const warning = Promise.resolve(
    src ? Internal.maybeWarnAboutRebaseOffWarm?.(src, destBase) : src,
  );
  if (await warning) {
    const buttons = [
      t('Opt Out of Future Warnings'),
      {label: t('Cancel'), primary: true},
      t('Continue Anyway'),
    ];
    const answer = await showModal({
      type: 'confirm',
      buttons,
      title: <T>Move off Warm Commit</T>,
      message: t(
        Internal.warnAboutRebaseOffWarmReason ??
          "The commit you're on is a warmed up commit. Moving off will cause slower builds and performance.\n" +
            "It's recommended to rebase your changes onto the warmed up commit instead.\n" +
            "If you need fresher changes, it's recommended to reserve a new OD and work off the warm commit.\n" +
            'Do you want to continue anyway?',
      ),
    });
    const userEnv = (await Internal.getDevEnvType?.()) ?? 'NotImplemented';
    const cwd = readAtom(repoRelativeCwd);
    tracker.track('WarnAboutRebaseOffWarm', {
      extras: {
        userAction: answer,
        envType: userEnv,
        cwd,
      },
    });
    if (answer === buttons[0]) {
      writeAtom(rebaseOffWarmWarningEnabled, false);
      return WarningCheckResult.PASS;
    }
    return answer === buttons[2] ? WarningCheckResult.BYPASS : WarningCheckResult.FAIL;
  }

  return WarningCheckResult.PASS;
}

async function maybeWarnAboutRebaseOntoMaster(commit: CommitInfo): Promise<WarningCheckResult> {
  const isRebaseOntoMasterWarningEnabled = readAtom(rebaseOntoMasterWarningEnabled);
  if (!isRebaseOntoMasterWarningEnabled) {
    return WarningCheckResult.PASS;
  }

  const dag = readAtom(dagWithPreviews);
  const src = findPublicBaseAncestor(dag);
  const destBase = findPublicBaseAncestor(dag, commit.hash);

  if (!destBase) {
    // can't determine if we can show warning
    return Promise.resolve(WarningCheckResult.PASS);
  }

  const warning = Promise.resolve(
    src ? Internal.maybeWarnAboutRebaseOntoMaster?.(src, destBase) : false,
  );

  if (await warning) {
    const buttons = [
      t('Opt Out of Future Warnings'),
      {label: t('Cancel'), primary: true},
      t('Continue Anyway'),
    ];
    const answer = await showModal({
      type: 'confirm',
      buttons,
      title: <T>Rebase onto Master Warning</T>,
      message: t(
        Internal.warnAboutRebaseOntoMasterReason ??
          'You are about to rebase directly onto master/main. ' +
            'This is generally not recommended as it can cause unexpected failures and slower builds. ' +
            'Consider rebasing onto a stable or warm branch instead. ' +
            'Do you want to continue anyway?',
      ),
    });
    const userEnv = (await Internal.getDevEnvType?.()) ?? 'NotImplemented';
    const cwd = readAtom(repoRelativeCwd);
    tracker.track('WarnAboutRebaseOntoMaster', {
      extras: {
        userAction: answer,
        envType: userEnv,
        cwd,
      },
    });
    if (answer === buttons[0]) {
      writeAtom(rebaseOntoMasterWarningEnabled, false);
      return WarningCheckResult.PASS;
    }
    return answer === buttons[2] ? WarningCheckResult.BYPASS : WarningCheckResult.FAIL;
  }

  return WarningCheckResult.PASS;
}

async function gotoAction(runOperation: ReturnType<typeof useRunOperation>, commit: CommitInfo) {
  const shouldProceed = await runWarningChecks([
    () => maybeWarnAboutRebaseOntoMaster(commit),
    () => maybeWarnAboutOldDestination(commit),
    () => maybeWarnAboutRebaseOffWarm(commit),
  ]);

  if (!shouldProceed) {
    return;
  }

  const dest =
    // If the commit has a remote bookmark, use that instead of the hash. This is easier to read in the command history
    // and works better with optimistic state
    commit.remoteBookmarks.length > 0
      ? succeedableRevset(commit.remoteBookmarks[0])
      : latestSuccessorUnlessExplicitlyObsolete(commit);
  runOperation(new GotoOperation(dest));
  // Instead of propagating, ensure we remove the selection, so we view the new head commit by default
  // (since the head commit is the default thing shown in the sidebar)
  writeAtom(selectedCommits, new Set());
}

const ObsoleteTip = React.memo(ObsoleteTipInner);

/**
 * Runs a series of validation checks sequentially. Returns true if all checks pass
 * or the user manually bypassed a warning, otherwise returns false if any check fails.
 */
async function runWarningChecks(
  checks: Array<() => Promise<WarningCheckResult>>,
): Promise<boolean> {
  for (const check of checks) {
    // eslint-disable-next-line no-await-in-loop
    const result = await check();
    if (result !== WarningCheckResult.PASS) {
      return result === WarningCheckResult.BYPASS;
    }
  }
  return true;
}

async function handleRebaseConfirmation(
  commit: CommitInfo,
  handlePreviewedOperation: (cancel: boolean) => void,
): Promise<void> {
  const shouldProceed = await runWarningChecks([
    () => maybeWarnAboutRebaseOntoMaster(commit),
    () => maybeWarnAboutDistantRebase(commit),
    () => maybeWarnAboutRebaseOffWarm(commit),
  ]);

  if (!shouldProceed) {
    return;
  }

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
}

async function maybeWarnAboutDistantRebase(commit: CommitInfo): Promise<WarningCheckResult> {
  const isDistantRebaseWarningEnabled = readAtom(distantRebaseWarningEnabled);
  if (!isDistantRebaseWarningEnabled) {
    return WarningCheckResult.PASS;
  }
  const dag = readAtom(dagWithPreviews);
  const onto = dag.get(commit.parents[0]);
  if (!onto) {
    return WarningCheckResult.PASS; // If there's no target commit, proceed without warning
  }
  const currentBase = findPublicBaseAncestor(dag);
  const destBase = findPublicBaseAncestor(dag, onto.hash);
  if (!currentBase || !destBase) {
    // can't determine if we can show warning
    return Promise.resolve(WarningCheckResult.PASS);
  }
  const warning = Promise.resolve(
    Internal.maybeWarnAboutDistantRebase?.(currentBase, destBase) ?? false,
  );

  if (await warning) {
    const buttons = [
      t('Opt Out of Future Warnings'),
      {label: t('Cancel'), primary: true},
      t('Rebase Anyway'),
    ];
    const answer = await showModal({
      type: 'confirm',
      buttons,
      title: <T>Distant Rebase Warning</T>,
      message: t(
        Internal.warnAboutDistantRebaseReason ??
          'The target commit is $age away from your current commit. ' +
            'Rebasing across a large time gap may cause slower builds and performance. ' +
            "It's recommended to rebase the destination commit(s) to the nearest stable or warm commit first and then attempt this rebase. " +
            'Do you want to `rebase` anyway?',
        {
          replace: {
            $age: relativeDate(onto.date, {reference: commit.date, useRelativeForm: true}),
          },
        },
      ),
    });
    const userEnv = (await Internal.getDevEnvType?.()) ?? 'NotImplemented';
    const cwd = readAtom(repoRelativeCwd);
    tracker.track('WarnAboutDistantRebase', {
      extras: {
        userAction: answer,
        envType: userEnv,
        cwd,
      },
    });
    if (answer === buttons[0]) {
      writeAtom(distantRebaseWarningEnabled, false);
      return WarningCheckResult.PASS;
    }
    return answer === buttons[2] ? WarningCheckResult.BYPASS : WarningCheckResult.FAIL;
  }

  return WarningCheckResult.PASS;
}
