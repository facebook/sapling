/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  CommitInfoMode,
  EditedMessage,
  EditedMessageUnlessOptimistic,
  FieldsBeingEdited,
} from './CommitInfoState';
import type {CommitInfo} from './types';
import type {Dispatch, ReactNode, SetStateAction} from 'react';

import {YouAreHere} from './Commit';
import {
  assertNonOptimistic,
  commitFieldsBeingEdited,
  commitMode,
  editedCommitMessages,
  hasUnsavedEditedCommitMessage,
} from './CommitInfoState';
import {OpenComparisonViewButton} from './ComparisonView/OpenComparisonViewButton';
import {Center} from './ComponentUtils';
import {numPendingImageUploads} from './ImageUpload';
import {Subtle} from './Subtle';
import {CommitInfoField} from './TextArea';
import {Tooltip} from './Tooltip';
import {ChangedFiles, deselectedUncommittedChanges, UncommittedChanges} from './UncommittedChanges';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {t, T} from './i18n';
import {AmendMessageOperation} from './operations/AmendMessageOperation';
import {AmendOperation} from './operations/AmendOperation';
import {CommitOperation} from './operations/CommitOperation';
import {GhStackSubmitOperation} from './operations/GhStackSubmitOperation';
import {PrSubmitOperation} from './operations/PrSubmitOperation';
import {SetConfigOperation} from './operations/SetConfigOperation';
import platform from './platform';
import {treeWithPreviews, uncommittedChangesWithPreviews} from './previews';
import {RelativeDate} from './relativeDate';
import {selectedCommits} from './selection';
import {repositoryInfo, useRunOperation} from './serverAPIState';
import {useModal} from './useModal';
import {assert, firstOfIterable} from './utils';
import {
  VSCodeBadge,
  VSCodeButton,
  VSCodeDivider,
  VSCodeLink,
  VSCodeRadio,
  VSCodeRadioGroup,
} from '@vscode/webview-ui-toolkit/react';
import React, {useEffect} from 'react';
import {useRecoilCallback, useRecoilState, useRecoilValue} from 'recoil';
import {ComparisonType} from 'shared/Comparison';
import {Icon} from 'shared/Icon';
import {unwrap} from 'shared/utils';

import './CommitInfo.css';

export function CommitInfoSidebar() {
  const selected = useRecoilValue(selectedCommits);
  const {treeMap, headCommit} = useRecoilValue(treeWithPreviews);

  // show selected commit, if there's exactly 1
  const selectedCommit =
    selected.size === 1 ? treeMap.get(unwrap(firstOfIterable(selected.values()))) : undefined;
  const commit = selectedCommit?.info ?? headCommit;

  if (commit == null) {
    return (
      <div className="commit-info-view" data-testid="commit-info-view-loading">
        <Center>
          <Icon icon="loading" />
        </Center>
      </div>
    );
  } else {
    return <CommitInfoDetails commit={commit} />;
  }
}

export function CommitInfoDetails({commit}: {commit: CommitInfo}) {
  const [mode, setMode] = useRecoilState(commitMode);
  const isCommitMode = commit.isHead && mode === 'commit';
  const [editedMessage, setEditedCommitMesage] = useRecoilState(
    editedCommitMessages(isCommitMode ? 'head' : commit.hash),
  );
  const uncommittedChanges = useRecoilValue(uncommittedChangesWithPreviews);

  const [fieldsBeingEdited, setFieldsBeingEdited] =
    useRecoilState<FieldsBeingEdited>(commitFieldsBeingEdited);

  const startEditingField = (field: 'title' | 'description') => {
    assert(
      editedMessage.type !== 'optimistic',
      'Cannot start editing fields when viewing optimistic commit',
    );
    setFieldsBeingEdited({...fieldsBeingEdited, [field]: true});
  };

  useEffect(() => {
    if (editedMessage.type === 'optimistic') {
      // invariant: if mode === 'commit', editedMessage.type !== 'optimistic'.
      assert(!isCommitMode, 'Should not be in commit mode while editedMessage.type is optimistic');

      // no fields are edited during optimistic state
      setFieldsBeingEdited({
        title: false,
        description: false,
      });
      return;
    }
    if (fieldsBeingEdited.forceWhileOnHead && commit.isHead) {
      // `forceWhileOnHead` is used to allow fields to be marked as edited externally,
      // even though they would get reset here after rendering.
      // This will get reset when the user cancels or changes to a different commit.
      return;
    }
    // If the selected commit is changed, the fields being edited should reset;
    // except for fields that are being edited on this commit, too
    setFieldsBeingEdited({
      title: isCommitMode || editedMessage.title !== commit.title,
      description: isCommitMode || editedMessage.description !== commit.description,
    });

    // We only want to recompute this when the commit/mode changes.
    // we expect the edited message to change constantly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [commit.hash, isCommitMode]);

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
      <div className="commit-info-view-main-content">
        {fieldsBeingEdited.title ? (
          <Section className="commit-info-title-field-section">
            <SmallCapsTitle>
              <Icon icon="milestone" />
              <T>Title</T>
            </SmallCapsTitle>
            <CommitInfoField
              which="title"
              autoFocus={true}
              editedMessage={assertNonOptimistic(editedMessage)}
              setEditedCommitMessage={setEditedCommitMesage}
              // remount this component if we switch commit mode
              key={mode}
            />
          </Section>
        ) : (
          <>
            <ClickToEditField
              startEditingField={
                editedMessage.type === 'optimistic' ? undefined : startEditingField
              }
              which="title">
              <span>{commit.title}</span>
              {editedMessage.type === 'optimistic' ? null : (
                <span className="hover-edit-button">
                  <Icon icon="edit" />
                </span>
              )}
            </ClickToEditField>
            <CommitTitleByline commit={commit} />
          </>
        )}
        {fieldsBeingEdited.description ? (
          <Section>
            <SmallCapsTitle>
              <Icon icon="note" />
              <T>Description</T>
            </SmallCapsTitle>
            <CommitInfoField
              which="description"
              autoFocus={!fieldsBeingEdited.title}
              editedMessage={assertNonOptimistic(editedMessage)}
              setEditedCommitMessage={setEditedCommitMesage}
              // remount this component if we switch commit mode
              key={mode}
            />
          </Section>
        ) : (
          <Section>
            <ClickToEditField
              startEditingField={
                editedMessage.type === 'optimistic' ? undefined : startEditingField
              }
              which="description">
              <SmallCapsTitle>
                <Icon icon="note" />
                <T>Description</T>
                <span className="hover-edit-button">
                  <Icon icon="edit" />
                </span>
              </SmallCapsTitle>
              {commit.description ? (
                <div>{commit.description}</div>
              ) : (
                <span className="empty-description subtle">
                  {editedMessage.type === 'optimistic' ? (
                    <>
                      <T>No description</T>
                    </>
                  ) : (
                    <>
                      <Icon icon="add" />
                      <T> Click to add description</T>
                    </>
                  )}
                </span>
              )}
            </ClickToEditField>
          </Section>
        )}
        <VSCodeDivider />
        {commit.isHead ? (
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
          <Section>
            <SmallCapsTitle>
              <T>Files Changed</T>
              <VSCodeBadge>{commit.totalFileCount}</VSCodeBadge>
            </SmallCapsTitle>
            <div className="changed-file-list">
              <OpenComparisonViewButton
                comparison={{type: ComparisonType.Committed, hash: commit.hash}}
              />
              <ChangedFiles files={commit.filesSample} showFileActions={false} />
            </div>
          </Section>
        )}
      </div>
      <div className="commit-info-view-toolbar-bottom">
        <ActionsBar
          commit={commit}
          editedMessage={editedMessage}
          fieldsBeingEdited={fieldsBeingEdited}
          setFieldsBeingEdited={setFieldsBeingEdited}
          isCommitMode={isCommitMode}
          setMode={setMode}
        />
      </div>
    </div>
  );
}

function ActionsBar({
  commit,
  editedMessage,
  fieldsBeingEdited,
  setFieldsBeingEdited,
  isCommitMode,
  setMode,
}: {
  commit: CommitInfo;
  editedMessage: EditedMessageUnlessOptimistic;
  fieldsBeingEdited: FieldsBeingEdited;
  setFieldsBeingEdited: Dispatch<SetStateAction<FieldsBeingEdited>>;
  isCommitMode: boolean;
  setMode: (mode: CommitInfoMode) => unknown;
}) {
  const isAnythingBeingEdited = fieldsBeingEdited.title || fieldsBeingEdited.description;
  const uncommittedChanges = useRecoilValue(uncommittedChangesWithPreviews);
  const deselected = useRecoilValue(deselectedUncommittedChanges);
  const anythingToCommit =
    !(deselected.size > 0 && deselected.size === uncommittedChanges.length) &&
    ((!isCommitMode && isAnythingBeingEdited) || uncommittedChanges.length > 0);

  const provider = useRecoilValue(codeReviewProvider);
  const [repoInfo, setRepoInfo] = useRecoilState(repositoryInfo);

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
        setFieldsBeingEdited({title: false, description: false});
      },
  );
  const runOperation = useRunOperation();
  const doAmendOrCommit = () => {
    const filesToCommit =
      deselected.size === 0
        ? // all files
          undefined
        : // only files not unchecked
          uncommittedChanges.filter(file => !deselected.has(file.path)).map(file => file.path);
    runOperation(
      isCommitMode
        ? new CommitOperation(assertNonOptimistic(editedMessage), commit.hash, filesToCommit)
        : new AmendOperation(filesToCommit, assertNonOptimistic(editedMessage)),
    );
    clearEditedCommitMessage(/* skip confirmation */ true);
    // reset to amend mode now that the commit has been made
    setMode('amend');
    deselectIfHeadIsSelected();
  };

  const showOptionModal = useModal();

  const codeReviewProviderName =
    repoInfo?.type === 'success' ? repoInfo.codeReviewSystem.type : 'unknown';
  const canSubmitWithCodeReviewProvider =
    codeReviewProviderName !== 'none' && codeReviewProviderName !== 'unknown';

  const ongoingImageUploads = useRecoilValue(numPendingImageUploads);
  const areImageUploadsOngoing = ongoingImageUploads > 0;

  return (
    <div className="commit-info-actions-bar" data-testid="commit-info-actions-bar">
      {isAnythingBeingEdited && !isCommitMode ? (
        <VSCodeButton appearance="secondary" onClick={() => clearEditedCommitMessage()}>
          <T>Cancel</T>
        </VSCodeButton>
      ) : null}

      {commit.isHead ? (
        <Tooltip
          title={
            areImageUploadsOngoing
              ? t('Image uploads are still pending')
              : isCommitMode
              ? deselected.size === 0
                ? t('No changes to commit')
                : t('No selected changes to commit')
              : deselected.size === 0
              ? t('No changes to amend')
              : t('No selected changes to amend')
          }
          trigger={areImageUploadsOngoing || !anythingToCommit ? 'hover' : 'disabled'}>
          <VSCodeButton
            appearance="secondary"
            disabled={!anythingToCommit || editedMessage == null || areImageUploadsOngoing}
            onClick={doAmendOrCommit}>
            {isCommitMode ? <T>Commit</T> : <T>Amend</T>}
          </VSCodeButton>
        </Tooltip>
      ) : (
        <Tooltip
          title={t('Image uploads are still pending')}
          trigger={areImageUploadsOngoing ? 'hover' : 'disabled'}>
          <VSCodeButton
            appearance="secondary"
            disabled={!isAnythingBeingEdited || editedMessage == null || areImageUploadsOngoing}
            onClick={() => {
              runOperation(
                new AmendMessageOperation(commit.hash, assertNonOptimistic(editedMessage)),
              );
              clearEditedCommitMessage(/* skip confirmation */ true);
            }}>
            <T>Amend Message</T>
          </VSCodeButton>
        </Tooltip>
      )}
      {commit.isHead ? (
        <Tooltip
          title={
            areImageUploadsOngoing
              ? t('Image uploads are still pending')
              : canSubmitWithCodeReviewProvider
              ? t('Submit for code review with $provider', {
                  replace: {$provider: codeReviewProviderName},
                })
              : t('Submitting for code review is currently only supported for GitHub-backed repos')
          }
          placement="top">
          <VSCodeButton
            disabled={!canSubmitWithCodeReviewProvider || areImageUploadsOngoing}
            onClick={async () => {
              if (anythingToCommit) {
                doAmendOrCommit();
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
                          You can configure Sapling to use either $pr or $ghstack to submit for code
                          review on GitHub.
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
                runOperation(
                  new SetConfigOperation('local', 'github.preferred_submit_command', answer),
                );
                setRepoInfo(info => ({
                  ...unwrap(info),
                  preferredSubmitCommand: answer,
                }));
                // setRepoInfo updates `provider`, but we still have a stale reference in this callback.
                // So this one time, we need to manually run the new submit command.
                // Future submit calls can delegate to provider.submitOperation();
                runOperation(
                  answer === 'ghstack' ? new GhStackSubmitOperation() : new PrSubmitOperation(),
                );
                return;
              }
              runOperation(unwrap(provider).submitOperation());
            }}>
            {anythingToCommit ? (
              isCommitMode ? (
                <T>Commit and Submit</T>
              ) : (
                <T>Amend and Submit</T>
              )
            ) : (
              <T>Submit</T>
            )}
          </VSCodeButton>
        </Tooltip>
      ) : null}
    </div>
  );
}

function CommitTitleByline({commit}: {commit: CommitInfo}) {
  const createdByInfo = (
    // TODO: determine if you're the author to say "you"
    <T replace={{$author: commit.author}}>Created by $author</T>
  );
  return (
    <Subtle className="commit-info-title-byline">
      {commit.isHead ? <YouAreHere hideSpinner /> : null}
      <OverflowEllipsis shrink>
        <Tooltip trigger="hover" component={() => createdByInfo}>
          {createdByInfo}
        </Tooltip>
      </OverflowEllipsis>
      <OverflowEllipsis>
        <Tooltip trigger="hover" title={commit.date.toLocaleString()}>
          <RelativeDate date={commit.date} />
        </Tooltip>
      </OverflowEllipsis>
    </Subtle>
  );
}

function OverflowEllipsis({children, shrink}: {children: ReactNode; shrink?: boolean}) {
  return <div className={`overflow-ellipsis${shrink ? ' overflow-shrink' : ''}`}>{children}</div>;
}

function SmallCapsTitle({children}: {children: ReactNode}) {
  return <div className="commit-info-small-title">{children}</div>;
}

function Section({
  children,
  className,
  ...rest
}: React.DetailedHTMLProps<React.HTMLAttributes<HTMLElement>, HTMLElement>) {
  return (
    <section {...rest} className={'commit-info-section' + (className ? ' ' + className : '')}>
      {children}
    </section>
  );
}

function ClickToEditField({
  children,
  startEditingField,
  which,
}: {
  children: ReactNode;
  /** function to run when you click to edit. If null, the entire field will be non-editable. */
  startEditingField?: (which: keyof EditedMessage) => void;
  which: keyof EditedMessage;
}) {
  const editable = startEditingField != null;
  return (
    <div
      className={`commit-info-rendered-${which}${editable ? '' : ' non-editable'}`}
      data-testid={`commit-info-rendered-${which}`}
      onClick={
        startEditingField != null
          ? () => {
              startEditingField(which);
            }
          : undefined
      }
      onKeyPress={
        startEditingField != null
          ? e => {
              if (e.key === 'Enter') {
                startEditingField(which);
              }
            }
          : undefined
      }
      tabIndex={0}>
      {children}
    </div>
  );
}
