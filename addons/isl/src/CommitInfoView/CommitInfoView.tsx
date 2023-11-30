/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, DiffId} from '../types';
import type {CommitInfoMode, EditedMessageUnlessOptimistic} from './CommitInfoState';
import type {CommitMessageFields, FieldConfig, FieldsBeingEdited} from './types';

import {Banner, BannerKind} from '../Banner';
import {ChangedFilesWithFetching} from '../ChangedFilesWithFetching';
import serverAPI from '../ClientToServerAPI';
import {Commit} from '../Commit';
import {OpenComparisonViewButton} from '../ComparisonView/OpenComparisonViewButton';
import {Center} from '../ComponentUtils';
import {HighlightCommitsWhileHovering} from '../HighlightedCommits';
import {numPendingImageUploads} from '../ImageUpload';
import {OperationDisabledButton} from '../OperationDisabledButton';
import {SubmitUpdateMessageInput} from '../SubmitUpdateMessageInput';
import {Subtle} from '../Subtle';
import {latestSuccessorUnlessExplicitlyObsolete} from '../SuccessionTracker';
import {SuggestedRebaseButton} from '../SuggestedRebase';
import {Tooltip} from '../Tooltip';
import {UncommittedChanges} from '../UncommittedChanges';
import {tracker} from '../analytics';
import {
  allDiffSummaries,
  codeReviewProvider,
  latestCommitMessageFields,
} from '../codeReview/CodeReviewInfo';
import {submitAsDraft, SubmitAsDraftCheckbox} from '../codeReview/DraftCheckbox';
import {FoldButton, useRunFoldPreview} from '../fold';
import {t, T} from '../i18n';
import {messageSyncingEnabledState, updateRemoteMessage} from '../messageSyncing';
import {AmendMessageOperation} from '../operations/AmendMessageOperation';
import {getAmendOperation} from '../operations/AmendOperation';
import {getCommitOperation} from '../operations/CommitOperation';
import {FOLD_COMMIT_PREVIEW_HASH_PREFIX} from '../operations/FoldOperation';
import {GhStackSubmitOperation} from '../operations/GhStackSubmitOperation';
import {PrSubmitOperation} from '../operations/PrSubmitOperation';
import {SetConfigOperation} from '../operations/SetConfigOperation';
import {useUncommittedSelection} from '../partialSelection';
import platform from '../platform';
import {CommitPreview, uncommittedChangesWithPreviews} from '../previews';
import {selectedCommits} from '../selection';
import {commitByHash, latestHeadCommit, repositoryInfo, useRunOperation} from '../serverAPIState';
import {succeedableRevset} from '../types';
import {useModal} from '../useModal';
import {assert, firstLine, firstOfIterable} from '../utils';
import {CommitInfoField} from './CommitInfoField';
import {
  forceNextCommitToEditAllFields,
  unsavedFieldsBeingEdited,
  diffUpdateMessagesState,
  commitInfoViewCurrentCommits,
  assertNonOptimistic,
  commitMode,
  editedCommitMessages,
  hasUnsavedEditedCommitMessage,
} from './CommitInfoState';
import {
  commitMessageFieldsToString,
  commitMessageFieldsSchema,
  parseCommitMessageFields,
  findFieldsBeingEdited,
  findEditedDiffNumber,
  applyEditedFields,
  editedMessageSubset,
  removeNoopEdits,
} from './CommitMessageFields';
import {CommitTitleByline, getTopmostEditedField, Section, SmallCapsTitle} from './utils';
import {
  VSCodeBadge,
  VSCodeButton,
  VSCodeDivider,
  VSCodeLink,
  VSCodeRadio,
  VSCodeRadioGroup,
} from '@vscode/webview-ui-toolkit/react';
import {useEffect} from 'react';
import {useRecoilCallback, useRecoilState, useRecoilValue} from 'recoil';
import {ComparisonType} from 'shared/Comparison';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {debounce} from 'shared/debounce';
import {notEmpty, unwrap} from 'shared/utils';

import './CommitInfoView.css';

export function CommitInfoSidebar() {
  const commitsToShow = useRecoilValue(commitInfoViewCurrentCommits);

  if (commitsToShow == null) {
    return (
      <div className="commit-info-view" data-testid="commit-info-view-loading">
        <Center>
          <Icon icon="loading" />
        </Center>
      </div>
    );
  } else {
    if (commitsToShow.length > 1) {
      return <MultiCommitInfo selectedCommits={commitsToShow} />;
    }

    // only one commit selected
    return <CommitInfoDetails commit={commitsToShow[0]} />;
  }
}

export function MultiCommitInfo({selectedCommits}: {selectedCommits: Array<CommitInfo>}) {
  const provider = useRecoilValue(codeReviewProvider);
  const diffSummaries = useRecoilValue(allDiffSummaries);
  const shouldSubmitAsDraft = useRecoilValue(submitAsDraft);
  const commitsWithDiffs = selectedCommits.filter(commit => commit.diffId != null);
  const [updateMessage, setUpdateMessage] = useRecoilState(
    // Combine hashes to key the typed update message.
    // This is kind of volatile, since if you change your selection at all, the message will be cleared.
    diffUpdateMessagesState(selectedCommits.map(commit => commit.hash).join(',')),
  );
  const submittable =
    (diffSummaries.value != null
      ? provider?.getSubmittableDiffs(selectedCommits, diffSummaries.value)
      : undefined) ?? [];
  return (
    <div className="commit-info-view-multi-commit" data-testid="commit-info-view">
      <strong className="commit-list-header">
        <Icon icon="layers" size="M" />
        <T replace={{$num: selectedCommits.length}}>$num Commits Selected</T>
      </strong>
      <VSCodeDivider />
      <div className="commit-list">
        {selectedCommits.map(commit => (
          <Commit
            key={commit.hash}
            commit={commit}
            hasChildren={false}
            previewType={CommitPreview.NON_ACTIONABLE_COMMIT}
          />
        ))}
      </div>
      <div className="commit-info-actions-bar">
        <div className="commit-info-actions-bar-row">
          <SuggestedRebaseButton
            sources={selectedCommits.map(commit => succeedableRevset(commit.hash))}
          />
        </div>
        {commitsWithDiffs.length === 0 ? null : (
          <SubmitUpdateMessageInput commits={selectedCommits} />
        )}
        <div className="commit-info-actions-bar-row">
          <FoldButton />
        </div>
        <div className="commit-info-actions-bar-left">
          <SubmitAsDraftCheckbox commitsToBeSubmit={selectedCommits} />
        </div>
        <div className="commit-info-actions-bar-right">
          {submittable.length === 0 ? null : (
            <HighlightCommitsWhileHovering toHighlight={submittable}>
              <OperationDisabledButton
                contextKey={'submit-selected'}
                appearance="secondary"
                runOperation={() => {
                  // clear update message on submit
                  setUpdateMessage('');
                  return unwrap(provider).submitOperation(selectedCommits, {
                    draft: shouldSubmitAsDraft,
                    updateMessage: updateMessage || undefined,
                  });
                }}>
                <T>Submit Selected Commits</T>
              </OperationDisabledButton>
            </HighlightCommitsWhileHovering>
          )}
        </div>
      </div>
    </div>
  );
}

const debouncedDiffFetch = debounce(
  (diffId: string) => {
    serverAPI.postMessage({
      type: 'fetchDiffSummaries',
      diffIds: [diffId],
    });
  },
  10_000,
  undefined,
  /* leading */ true,
);
function useDebounceFetchDiffDetails(diffId?: string) {
  useEffect(() => {
    // reset debouncing any time the current diff changes
    debouncedDiffFetch.reset();
  }, [diffId]);
  if (diffId != null) {
    debouncedDiffFetch(diffId);
  }
}

export function CommitInfoDetails({commit}: {commit: CommitInfo}) {
  const [mode, setMode] = useRecoilState(commitMode);
  const isCommitMode = commit.isHead && mode === 'commit';
  const hashOrHead = isCommitMode ? 'head' : commit.hash;
  const [editedMessage, setEditedCommitMessage] = useRecoilState(editedCommitMessages(hashOrHead));
  const uncommittedChanges = useRecoilValue(uncommittedChangesWithPreviews);
  const schema = useRecoilValue(commitMessageFieldsSchema);

  const isFoldPreview = commit.hash.startsWith(FOLD_COMMIT_PREVIEW_HASH_PREFIX);
  const isOptimistic =
    useRecoilValue(commitByHash(commit.hash)) == null && !isCommitMode && !isFoldPreview;

  const isPublic = mode === 'amend' && commit.phase === 'public';

  const fieldsBeingEdited = useRecoilValue(unsavedFieldsBeingEdited(hashOrHead));

  useDebounceFetchDiffDetails(commit.diffId);

  const [forceEditAll, setForceEditAll] = useRecoilState(forceNextCommitToEditAllFields);

  useEffect(() => {
    if (editedMessage.type === 'optimistic') {
      // invariant: if mode === 'commit', editedMessage.type !== 'optimistic'.
      assert(!isCommitMode, 'Should not be in commit mode while editedMessage.type is optimistic');
      return;
    }
    if (isCommitMode && commit.isHead) {
      // no use resetting edited state for commit mode, where it's always being edited.
      return;
    }

    if (!forceEditAll) {
      // If the selected commit is changed, the fields being edited should slim down to only fields
      // that are meaningfully edited on the new commit.
      if (Object.keys(editedMessage.fields).length > 0) {
        const trimmedEdits = removeNoopEdits(schema, parsedFields, editedMessage.fields);
        if (Object.keys(trimmedEdits).length !== Object.keys(editedMessage.fields).length) {
          setEditedCommitMessage({fields: trimmedEdits});
        }
      }
    }
    setForceEditAll(false);

    // We only want to recompute this when the commit/mode changes.
    // we expect the edited message to change constantly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [commit.hash, isCommitMode]);

  const parsedFields = useRecoilValue(latestCommitMessageFields(hashOrHead));

  const startEditingField = (field: string) => {
    assert(
      editedMessage.type !== 'optimistic',
      'Cannot start editing fields when viewing optimistic commit',
    );
    // Set the latest message value for the edited message of this field.
    // fieldsBeingEdited is derived from this.
    setEditedCommitMessage(last =>
      last.type === 'optimistic'
        ? last
        : {
            fields: {
              ...last.fields,
              [field]: parsedFields[field],
            },
          },
    );
  };

  const topmostEditedField = getTopmostEditedField(schema, fieldsBeingEdited);

  return (
    <div className="commit-info-view" data-testid="commit-info-view">
      {!commit.isHead ? null : (
        <div className="commit-info-view-toolbar-top" data-testid="commit-info-toolbar-top">
          <VSCodeRadioGroup
            value={mode}
            onChange={e => setMode((e.target as HTMLOptionElement).value as CommitInfoMode)}>
            <VSCodeRadio value="commit" checked={mode === 'commit'} tabIndex={0}>
              <T>Commit</T>
            </VSCodeRadio>
            <VSCodeRadio value="amend" checked={mode === 'amend'} tabIndex={0}>
              <T>Amend</T>
            </VSCodeRadio>
          </VSCodeRadioGroup>
        </div>
      )}

      <div
        className="commit-info-view-main-content"
        // remount this if we change to commit mode
        key={mode}>
        {schema
          .filter(field => mode !== 'commit' || field.type !== 'read-only')
          .map(field => {
            const setField = (newVal: string) =>
              setEditedCommitMessage(val =>
                val.type === 'optimistic'
                  ? val
                  : {
                      fields: {
                        ...val.fields,
                        [field.key]: field.type === 'field' ? [newVal] : newVal,
                      },
                    },
              );

            let editedFieldValue = editedMessage.fields?.[field.key];
            if (editedFieldValue == null && mode === 'commit' && commit.isHead) {
              // If the field is supposed to edited but not in the editedMessage,
              // it means we're loading from a blank slate. This is when we can load from the commit template.
              editedFieldValue = parsedFields[field.key];
            }

            return (
              <CommitInfoField
                key={field.key}
                field={field}
                content={parsedFields[field.key as keyof CommitMessageFields]}
                autofocus={topmostEditedField === field.key}
                readonly={isOptimistic || isPublic}
                isBeingEdited={fieldsBeingEdited[field.key]}
                startEditingField={() => startEditingField(field.key)}
                editedField={editedFieldValue}
                setEditedField={setField}
                extra={
                  mode !== 'commit' && field.key === 'Title' ? (
                    <>
                      <CommitTitleByline commit={commit} />
                      {isFoldPreview && <FoldPreviewBanner />}
                      <ShowingRemoteMessageBanner
                        commit={commit}
                        latestFields={parsedFields}
                        editedCommitMessageKey={isCommitMode ? 'head' : commit.hash}
                      />
                    </>
                  ) : undefined
                }
              />
            );
          })}
        <VSCodeDivider />
        {commit.isHead && !isPublic ? (
          <Section data-testid="changes-to-amend">
            <SmallCapsTitle>
              {isCommitMode ? <T>Changes to Commit</T> : <T>Changes to Amend</T>}
              <VSCodeBadge>{uncommittedChanges.length}</VSCodeBadge>
            </SmallCapsTitle>
            {uncommittedChanges.length === 0 ? (
              <Subtle>
                {isCommitMode ? <T>No changes to commit</T> : <T>No changes to amend</T>}
              </Subtle>
            ) : (
              <UncommittedChanges place={isCommitMode ? 'commit sidebar' : 'amend sidebar'} />
            )}
          </Section>
        ) : null}
        {isCommitMode ? null : (
          <Section data-testid="committed-changes">
            <SmallCapsTitle>
              <T>Files Changed</T>
              <VSCodeBadge>{commit.totalFileCount}</VSCodeBadge>
            </SmallCapsTitle>
            <div className="changed-file-list">
              <div className="button-row">
                <OpenComparisonViewButton
                  comparison={{type: ComparisonType.Committed, hash: commit.hash}}
                />
                <VSCodeButton
                  appearance="icon"
                  onClick={() => {
                    tracker.track('OpenAllFiles');
                    for (const file of commit.filesSample) {
                      platform.openFile(file.path);
                    }
                  }}>
                  <Icon icon="go-to-file" slot="start" />
                  <T>Open All Files</T>
                </VSCodeButton>
              </div>
              <ChangedFilesWithFetching commit={commit} />{' '}
            </div>
          </Section>
        )}
      </div>
      {!isPublic && (
        <div className="commit-info-view-toolbar-bottom">
          {isFoldPreview ? (
            <FoldPreviewActions />
          ) : (
            <ActionsBar
              commit={commit}
              latestMessage={parsedFields}
              editedMessage={editedMessage}
              fieldsBeingEdited={fieldsBeingEdited}
              isCommitMode={isCommitMode}
              setMode={setMode}
            />
          )}
        </div>
      )}
    </div>
  );
}

/**
 * Two parsed commit messages are considered unchanged if all the textareas (summary, test plan) are unchanged.
 * This avoids marking tiny changes like adding a reviewer as substatively changing the message.
 */
function areTextFieldsUnchanged(
  schema: Array<FieldConfig>,
  a: CommitMessageFields,
  b: CommitMessageFields,
) {
  for (const field of schema) {
    if (field.type === 'textarea') {
      if (a[field.key] !== b[field.key]) {
        return false;
      }
    }
  }
  return true;
}

function FoldPreviewBanner() {
  return (
    <Banner
      kind={BannerKind.green}
      icon={<Icon icon="info" />}
      tooltip={t(
        'This is the commit message after combining these commits with the fold command. ' +
          'You can edit this message before confirming and running fold.',
      )}>
      <T>Previewing result of combined commits</T>
    </Banner>
  );
}

function ShowingRemoteMessageBanner({
  commit,
  latestFields,
  editedCommitMessageKey,
}: {
  commit: CommitInfo;
  latestFields: CommitMessageFields;
  editedCommitMessageKey: string;
}) {
  const provider = useRecoilValue(codeReviewProvider);
  const schema = useRecoilValue(commitMessageFieldsSchema);
  const runOperation = useRunOperation();
  const syncingEnabled = useRecoilValue(messageSyncingEnabledState);

  const loadLocalMessage = useRecoilCallback(({set}) => () => {
    const originalFields = parseCommitMessageFields(schema, commit.title, commit.description);
    const beingEdited = findFieldsBeingEdited(schema, originalFields, latestFields);

    set(editedCommitMessages(editedCommitMessageKey), previous => {
      if (previous.type === 'optimistic') {
        return previous;
      }
      return editedMessageSubset(originalFields, beingEdited);
    });
  });

  const contextMenu = useContextMenu(() => {
    return [
      {
        label: <T>Load local commit message instead</T>,
        onClick: loadLocalMessage,
      },
      {
        label: <T>Sync local commit to match remote</T>,
        onClick: () => {
          runOperation(
            new AmendMessageOperation(
              succeedableRevset(commit.hash),
              commitMessageFieldsToString(schema, latestFields),
            ),
          );
        },
      },
    ];
  });

  if (!syncingEnabled || !provider) {
    return null;
  }

  const originalFields = parseCommitMessageFields(schema, commit.title, commit.description);

  if (areTextFieldsUnchanged(schema, originalFields, latestFields)) {
    return null;
  }
  return (
    <>
      <Banner
        icon={<Icon icon="info" />}
        tooltip={t(
          'Viewing the newer commit message from $provider. This message will be used when your code is landed. You can also load the local message instead.',
          {replace: {$provider: provider.label}},
        )}
        buttons={
          <VSCodeButton
            appearance="icon"
            data-testid="message-sync-banner-context-menu"
            onClick={e => {
              contextMenu(e);
            }}>
            <Icon icon="ellipsis" />
          </VSCodeButton>
        }>
        <T replace={{$provider: provider.label}}>Showing latest commit message from $provider</T>
      </Banner>
    </>
  );
}

function FoldPreviewActions() {
  const [cancel, run] = useRunFoldPreview();
  return (
    <div className="commit-info-actions-bar" data-testid="commit-info-actions-bar">
      <div className="commit-info-actions-bar-right">
        <VSCodeButton appearance="secondary" onClick={cancel}>
          <T>Cancel</T>
        </VSCodeButton>
        <VSCodeButton appearance="primary" onClick={run}>
          <T>Run Combine</T>
        </VSCodeButton>
      </div>
    </div>
  );
}

function ActionsBar({
  commit,
  latestMessage,
  editedMessage,
  fieldsBeingEdited,
  isCommitMode,
  setMode,
}: {
  commit: CommitInfo;
  latestMessage: CommitMessageFields;
  editedMessage: EditedMessageUnlessOptimistic;
  fieldsBeingEdited: FieldsBeingEdited;
  isCommitMode: boolean;
  setMode: (mode: CommitInfoMode) => unknown;
}) {
  const isAnythingBeingEdited = Object.values(fieldsBeingEdited).some(Boolean);
  const uncommittedChanges = useRecoilValue(uncommittedChangesWithPreviews);
  const selection = useUncommittedSelection();
  const anythingToCommit =
    !selection.isNothingSelected() &&
    ((!isCommitMode && isAnythingBeingEdited) || uncommittedChanges.length > 0);

  const provider = useRecoilValue(codeReviewProvider);
  const [repoInfo, setRepoInfo] = useRecoilState(repositoryInfo);
  const diffSummaries = useRecoilValue(allDiffSummaries);
  const shouldSubmitAsDraft = useRecoilValue(submitAsDraft);
  const schema = useRecoilValue(commitMessageFieldsSchema);
  const headCommit = useRecoilValue(latestHeadCommit);

  const [updateMessage, setUpdateMessage] = useRecoilState(diffUpdateMessagesState(commit.hash));

  const messageSyncEnabled = useRecoilValue(messageSyncingEnabledState);

  // after committing/amending, if you've previously selected the head commit,
  // we should show you the newly amended/committed commit instead of the old one.
  const deselectIfHeadIsSelected = useRecoilCallback(({snapshot, reset}) => () => {
    if (!commit.isHead) {
      return;
    }
    const selected = snapshot.getLoadable(selectedCommits).valueMaybe();
    // only reset if selection exactly matches our expectation
    if (selected && selected.size === 1 && firstOfIterable(selected.values()) === commit.hash) {
      reset(selectedCommits);
    }
  });

  const clearEditedCommitMessage = useRecoilCallback(
    ({snapshot, reset}) =>
      async (skipConfirmation?: boolean) => {
        if (!skipConfirmation) {
          const hasUnsavedEditsLoadable = snapshot.getLoadable(
            hasUnsavedEditedCommitMessage(isCommitMode ? 'head' : commit.hash),
          );
          const hasUnsavedEdits = hasUnsavedEditsLoadable.valueMaybe() === true;
          if (hasUnsavedEdits) {
            const confirmed = await platform.confirm(
              t('Are you sure you want to discard your edited message?'),
            );
            if (confirmed === false) {
              return;
            }
          }
        }

        reset(editedCommitMessages(isCommitMode ? 'head' : commit.hash));
      },
  );
  const doAmendOrCommit = () => {
    const updatedMessage = applyEditedFields(
      latestMessage,
      assertNonOptimistic(editedMessage).fields,
    );
    const message = commitMessageFieldsToString(schema, updatedMessage);
    const headHash = headCommit?.hash ?? '.';
    const allFiles = uncommittedChanges.map(file => file.path);

    const operation = isCommitMode
      ? getCommitOperation(message, headHash, selection.selection, allFiles)
      : getAmendOperation(message, headHash, selection.selection, allFiles);

    selection.discardPartialSelections();

    clearEditedCommitMessage(/* skip confirmation */ true);
    // reset to amend mode now that the commit has been made
    setMode('amend');
    deselectIfHeadIsSelected();

    return operation;
  };

  const showOptionModal = useModal();

  const codeReviewProviderName = provider?.label;
  const codeReviewProviderType =
    repoInfo?.type === 'success' ? repoInfo.codeReviewSystem.type : 'unknown';
  const canSubmitWithCodeReviewProvider =
    codeReviewProviderType !== 'none' && codeReviewProviderType !== 'unknown';
  const submittable =
    diffSummaries.value && provider?.getSubmittableDiffs([commit], diffSummaries.value);
  const canSubmitIndividualDiffs = submittable && submittable.length > 0;

  const ongoingImageUploads = useRecoilValue(numPendingImageUploads);
  const areImageUploadsOngoing = ongoingImageUploads > 0;

  // Generally "Amend"/"Commit" for head commit, but if there's no changes while amending, just use "Amend message"
  const showCommitOrAmend =
    commit.isHead && (isCommitMode || anythingToCommit || !isAnythingBeingEdited);

  const onCommitFormSubmit = platform.onCommitFormSubmit;

  return (
    <div className="commit-info-actions-bar" data-testid="commit-info-actions-bar">
      {isCommitMode || commit.diffId == null ? null : (
        <SubmitUpdateMessageInput commits={[commit]} />
      )}
      <div className="commit-info-actions-bar-left">
        <SubmitAsDraftCheckbox commitsToBeSubmit={isCommitMode ? [] : [commit]} />
      </div>
      <div className="commit-info-actions-bar-right">
        {isAnythingBeingEdited && !isCommitMode ? (
          <VSCodeButton appearance="secondary" onClick={() => clearEditedCommitMessage()}>
            <T>Cancel</T>
          </VSCodeButton>
        ) : null}

        {showCommitOrAmend ? (
          <Tooltip
            title={
              areImageUploadsOngoing
                ? t('Image uploads are still pending')
                : isCommitMode
                ? selection.isEverythingSelected()
                  ? t('No changes to commit')
                  : t('No selected changes to commit')
                : selection.isEverythingSelected()
                ? t('No changes to amend')
                : t('No selected changes to amend')
            }
            trigger={areImageUploadsOngoing || !anythingToCommit ? 'hover' : 'disabled'}>
            <OperationDisabledButton
              contextKey={isCommitMode ? 'commit' : 'amend'}
              appearance="secondary"
              disabled={!anythingToCommit || editedMessage == null || areImageUploadsOngoing}
              runOperation={async () => {
                if (!isCommitMode) {
                  const updatedMessage = applyEditedFields(
                    latestMessage,
                    assertNonOptimistic(editedMessage).fields,
                  );
                  const stringifiedMessage = commitMessageFieldsToString(schema, updatedMessage);
                  const diffId = findEditedDiffNumber(updatedMessage) ?? commit.diffId;
                  // if there's a diff attached, we should also update the remote message
                  if (messageSyncEnabled && diffId) {
                    const shouldAbort = await tryToUpdateRemoteMessage(
                      commit,
                      diffId,
                      stringifiedMessage,
                      showOptionModal,
                      'amend',
                    );
                    if (shouldAbort) {
                      return;
                    }
                  }
                }

                return doAmendOrCommit();
              }}>
              {isCommitMode ? <T>Commit</T> : <T>Amend</T>}
            </OperationDisabledButton>
          </Tooltip>
        ) : (
          <Tooltip
            title={
              areImageUploadsOngoing
                ? t('Image uploads are still pending')
                : !isAnythingBeingEdited
                ? t('No message edits to amend')
                : messageSyncEnabled && commit.diffId != null
                ? t(
                    'Amend the commit message with the newly entered message, then sync that message up to $provider.',
                    {replace: {$provider: codeReviewProviderName ?? 'remote'}},
                  )
                : t('Amend the commit message with the newly entered message.')
            }>
            <OperationDisabledButton
              contextKey={`amend-message-${commit.hash}`}
              appearance="secondary"
              data-testid="amend-message-button"
              disabled={!isAnythingBeingEdited || editedMessage == null || areImageUploadsOngoing}
              runOperation={async () => {
                const updatedMessage = applyEditedFields(
                  latestMessage,
                  assertNonOptimistic(editedMessage).fields,
                );
                const stringifiedMessage = commitMessageFieldsToString(schema, updatedMessage);
                const diffId = findEditedDiffNumber(updatedMessage) ?? commit.diffId;
                // if there's a diff attached, we should also update the remote message
                if (messageSyncEnabled && diffId) {
                  const shouldAbort = await tryToUpdateRemoteMessage(
                    commit,
                    diffId,
                    stringifiedMessage,
                    showOptionModal,
                    'amendMessage',
                  );
                  if (shouldAbort) {
                    return;
                  }
                }
                const operation = new AmendMessageOperation(
                  latestSuccessorUnlessExplicitlyObsolete(commit),
                  stringifiedMessage,
                );
                clearEditedCommitMessage(/* skip confirmation */ true);
                return operation;
              }}>
              <T>Amend Message</T>
            </OperationDisabledButton>
          </Tooltip>
        )}
        {(commit.isHead && (anythingToCommit || !isAnythingBeingEdited)) ||
        (!commit.isHead &&
          canSubmitIndividualDiffs &&
          // For non-head commits, "submit" doesn't update the message, which is confusing.
          // Just hide the submit button so you're encouraged to "amend message" first.
          !isAnythingBeingEdited) ? (
          <Tooltip
            title={
              areImageUploadsOngoing
                ? t('Image uploads are still pending')
                : canSubmitWithCodeReviewProvider
                ? t('Submit for code review with $provider', {
                    replace: {$provider: codeReviewProviderName ?? 'remote'},
                  })
                : t(
                    'Submitting for code review is currently only supported for GitHub-backed repos',
                  )
            }
            placement="top">
            <OperationDisabledButton
              contextKey={`submit-${commit.isHead ? 'head' : commit.hash}`}
              disabled={!canSubmitWithCodeReviewProvider || areImageUploadsOngoing}
              runOperation={async () => {
                if (onCommitFormSubmit !== undefined) {
                  onCommitFormSubmit();
                }

                let amendOrCommitOp;
                if (anythingToCommit) {
                  // TODO: we should also amend if there are pending commit message changes, and change the button
                  // to amend message & submit.
                  // Or just remove the submit button if you start editing since we'll update the remote message anyway...
                  amendOrCommitOp = doAmendOrCommit();
                }

                if (
                  repoInfo?.type === 'success' &&
                  repoInfo.codeReviewSystem.type === 'github' &&
                  repoInfo.preferredSubmitCommand == null
                ) {
                  const buttons = [t('Cancel') as 'Cancel', 'ghstack', 'pr'] as const;
                  const cancel = buttons[0];
                  const answer = await showOptionModal({
                    type: 'confirm',
                    icon: 'warning',
                    title: t('Preferred Code Review command not yet configured'),
                    message: (
                      <div className="commit-info-confirm-modal-paragraphs">
                        <div>
                          <T replace={{$pr: <code>sl pr</code>, $ghstack: <code>sl ghstack</code>}}>
                            You can configure Sapling to use either $pr or $ghstack to submit for
                            code review on GitHub.
                          </T>
                        </div>
                        <div>
                          <T
                            replace={{
                              $config: <code>github.preferred_submit_command</code>,
                            }}>
                            Each submit command has tradeoffs, due to how GitHub creates Pull
                            Requests. This can be controlled by the $config config.
                          </T>
                        </div>
                        <div>
                          <T>To continue, select a command to use to submit.</T>
                        </div>
                        <VSCodeLink
                          href="https://sapling-scm.com/docs/git/intro#pull-requests"
                          target="_blank">
                          <T>Learn More</T>
                        </VSCodeLink>
                      </div>
                    ),
                    buttons,
                  });
                  if (answer === cancel || answer == null) {
                    return;
                  }
                  const rememberConfigOp = new SetConfigOperation(
                    'local',
                    'github.preferred_submit_command',
                    answer,
                  );
                  setRepoInfo(info => ({
                    ...unwrap(info),
                    preferredSubmitCommand: answer,
                  }));
                  // setRepoInfo updates `provider`, but we still have a stale reference in this callback.
                  // So this one time, we need to manually run the new submit command.
                  // Future submit calls can delegate to provider.submitOperation();
                  const submitOp =
                    answer === 'ghstack'
                      ? new GhStackSubmitOperation({
                          draft: shouldSubmitAsDraft,
                        })
                      : new PrSubmitOperation({
                          draft: shouldSubmitAsDraft,
                        });

                  return [amendOrCommitOp, rememberConfigOp, submitOp].filter(notEmpty);
                }

                // Only do message sync if we're amending the local commit in some way.
                // If we're just doing a submit, we expect the message to have been synced previously
                // during another amend or amend message.
                const shouldUpdateMessage = !isCommitMode && messageSyncEnabled && anythingToCommit;

                const submitOp = unwrap(provider).submitOperation(
                  commit.isHead ? [] : [commit], // [] means to submit the head commit
                  {
                    draft: shouldSubmitAsDraft,
                    updateFields: shouldUpdateMessage,
                    updateMessage: updateMessage || undefined,
                  },
                );
                // clear out the update message now that we've used it to submit
                if (updateMessage) {
                  setUpdateMessage('');
                }

                return [amendOrCommitOp, submitOp].filter(notEmpty);
              }}>
              {commit.isHead && anythingToCommit ? (
                isCommitMode ? (
                  <T>Commit and Submit</T>
                ) : (
                  <T>Amend and Submit</T>
                )
              ) : (
                <T>Submit</T>
              )}
            </OperationDisabledButton>
          </Tooltip>
        ) : null}
      </div>
    </div>
  );
}

async function tryToUpdateRemoteMessage(
  commit: CommitInfo,
  diffId: DiffId,
  latestMessageString: string,
  showOptionModal: ReturnType<typeof useModal>,
  reason: 'amend' | 'amendMessage',
): Promise<boolean> {
  // TODO: we could skip the update if the new message matches the old one,
  // which is possible when amending changes without changing the commit message

  let optedOutOfSync = false;
  if (diffId !== commit.diffId) {
    const buttons = [
      t('Cancel') as 'Cancel',
      t('Use Remote Message'),
      t('Sync New Message'),
    ] as const;
    const cancel = buttons[0];
    const syncButton = buttons[2];
    const answer = await showOptionModal({
      type: 'confirm',
      icon: 'warning',
      title: t('Sync message for newly attached Diff?'),
      message: (
        <T>
          You're changing the attached Diff for this commit. Would you like you sync your new local
          message up to the remote Diff, or just use the existing remote message for this Diff?
        </T>
      ),
      buttons,
    });
    tracker.track('ConfirmSyncNewDiffNumber', {
      extras: {
        choice: answer,
      },
    });
    if (answer === cancel || answer == null) {
      return true; // abort
    }
    optedOutOfSync = answer !== syncButton;
  }
  if (!optedOutOfSync) {
    const title = firstLine(latestMessageString);
    const description = latestMessageString.slice(title.length);
    // don't wait for the update mutation to go through, just let it happen in parallel with the metaedit
    tracker
      .operation('SyncDiffMessageMutation', 'SyncMessageError', {extras: {reason}}, () =>
        updateRemoteMessage(diffId, title, description),
      )
      .catch(() => {
        // TODO: We should notify about this in the UI
      });
  }
  return false;
}
