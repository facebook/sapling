/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash} from './types';
import type {
  Dispatch,
  FormEvent,
  ForwardedRef,
  MutableRefObject,
  ReactNode,
  SetStateAction,
} from 'react';
import type {SetterOrUpdater} from 'recoil';

import {YouAreHere} from './Commit';
import {OpenComparisonViewButton} from './ComparisonView/OpenComparisonViewButton';
import {Icon} from './Icon';
import {Tooltip} from './Tooltip';
import {ChangedFiles, deselectedUncommittedChanges, UncommittedChanges} from './UncommittedChanges';
import {codeReviewProvider, repositoryInfo} from './codeReview/CodeReviewInfo';
import {t, T} from './i18n';
import {AmendMessageOperation} from './operations/AmendMessageOperation';
import {AmendOperation} from './operations/AmendOperation';
import {CommitOperation} from './operations/CommitOperation';
import {GhStackSubmitOperation} from './operations/GhStackSubmitOperation';
import {PrSubmitOperation} from './operations/PrSubmitOperation';
import {SetConfigOperation} from './operations/SetConfigOperation';
import {useOptionModal} from './optionsModal';
import platform from './platform';
import {treeWithPreviews, uncommittedChangesWithPreviews} from './previews';
import {RelativeDate} from './relativeDate';
import {selectedCommits} from './selection';
import {commitMessageTemplate, latestCommitTreeMap, useRunOperation} from './serverAPIState';
import {assert, firstOfIterable} from './utils';
import {
  VSCodeBadge,
  VSCodeButton,
  VSCodeDivider,
  VSCodeLink,
  VSCodeRadio,
  VSCodeRadioGroup,
  VSCodeTextArea,
} from '@vscode/webview-ui-toolkit/react';
import React, {forwardRef, useEffect, useRef} from 'react';
import {
  atom,
  atomFamily,
  selectorFamily,
  useRecoilCallback,
  useRecoilState,
  useRecoilValue,
} from 'recoil';
import {ComparisonType} from 'shared/Comparison';
import {unwrap} from 'shared/utils';

import './CommitInfo.css';

export type EditedMessage = {title: string; description: string};

/**
 * Which fields of the message should display as editors instead of rendered values.
 * This can be controlled outside of the commit info view, but it gets updated in an effect as well when commits are changed.
 * `forceWhileOnHead` can be used to prevent auto-updating when in amend mode to bypass this effect.
 * This value is removed whenever the next real update to the value is given.
 */
type FieldsBeingEdited = {title: boolean; description: boolean; forceWhileOnHead?: boolean};

type CommitInfoMode = 'commit' | 'amend';
type EditedMessageUnlessOptimistic =
  | (EditedMessage & {type?: undefined})
  | {type: 'optimistic'; title?: undefined; description?: undefined};

/**
 * Map of hash -> latest edited commit message, representing any changes made to the commit's message fields.
 * This also stores the state of new commit messages being written, keyed by "head" instead of a commit hash.
 * Even though messages are not edited by default, we can compute an initial state from the commit's original message,
 * which allows this state to be non-nullable which is very convenient. This shouldn't do any actual storage until it is written to.
 * Note: this state should be cleared when amending / committing / meta-editing.
 *
 * Note: since commits are looked up without optimistic state, its possible that we fail to look up the commit.
 * This would mean its a commit that only exists due to previews/optimitisc state,
 * for example the fake commit optimistically inserted as the new head while `commit` is running.
 * In such a state, we don't know the commit message we should use in the editor, nor do we have
 * a hash we could associate it with. For simplicity, the UI should prevent you from editing such commits' messages.
 * (TODO: hypothetically, we could track commit succession to take your partially edited message and persist it
 * once optimistic state resolves, but it would be complicated for not much benefit.)
 * We return a sentinel value without an edited message attached so the UI knows it cannot edit.
 * This optimistic value is never returned in commit mode.
 */
const editedCommitMessages = atomFamily<EditedMessageUnlessOptimistic, Hash | 'head'>({
  key: 'editedCommitMessages',
  default: selectorFamily({
    key: 'editedCommitMessages/defaults',
    get:
      hash =>
      ({get}) => {
        if (hash === 'head') {
          const template = get(commitMessageTemplate);
          return (
            template ?? {
              title: '',
              description: '',
            }
          );
        }
        // TODO: is there a better way we should derive `isOptimistic`
        // from `get(treeWithPreviews)`, rather than using non-previewed map?
        const map = get(latestCommitTreeMap);
        const info = map.get(hash)?.info;
        if (info == null) {
          return {type: 'optimistic'};
        }
        return {title: info.title, description: info.description};
      },
  }),
});

export const hasUnsavedEditedCommitMessage = selectorFamily<boolean, Hash | 'head'>({
  key: 'hasUnsavedEditedCommitMessage',
  get:
    hash =>
    ({get}) => {
      const edited = get(editedCommitMessages(hash));
      if (edited.type === 'optimistic') {
        return false;
      }
      if (hash === 'head') {
        return Boolean(edited.title || edited.description);
      }
      // TODO: use treeWithPreviews so this indicator is accurate on top of previews
      const original = get(latestCommitTreeMap).get(hash)?.info;
      return edited.title !== original?.title || edited.description !== original?.description;
    },
});

export const commitFieldsBeingEdited = atom<FieldsBeingEdited>({
  key: 'commitFieldsBeingEdited',
  default: {
    title: false,
    description: false,
  },
});

export const commitMode = atom<CommitInfoMode>({
  key: 'commitMode',
  default: 'amend',
});

/**
 * Throw if the edited message is of optimistic type.
 * We expect:
 *  - editedCommitMessage('head') should never be optimistic
 *  - editedCommitMessage(hashForCommitInTheTree) should not be optimistic
 *  - editedCommitMessage(hashForCommitNotInTheTree) should be optimistic
 */
function assertNonOptimistic(editedMessage: EditedMessageUnlessOptimistic): EditedMessage {
  if (editedMessage.type === 'optimistic') {
    throw new Error('Expected edited message to not be for optimistic commit');
  }
  return editedMessage;
}

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
        <Icon icon="loading" />
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
              <span className="subtle">
                {isCommitMode ? <T>No changes to commit</T> : <T>No changes to amend</T>}
              </span>
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
            <OpenComparisonViewButton
              comparison={{type: ComparisonType.Committed, hash: commit.hash}}
            />
            <ChangedFiles files={commit.filesSample} />
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

  const showOptionModal = useOptionModal();

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
            isCommitMode
              ? deselected.size === 0
                ? t('No changes to commit')
                : t('No selected changes to commit')
              : deselected.size === 0
              ? t('No changes to amend')
              : t('No selected changes to amend')
          }
          trigger={anythingToCommit ? 'disabled' : 'hover'}>
          <VSCodeButton
            appearance="secondary"
            disabled={!anythingToCommit || editedMessage == null}
            onClick={doAmendOrCommit}>
            {isCommitMode ? <T>Commit</T> : <T>Amend</T>}
          </VSCodeButton>
        </Tooltip>
      ) : (
        <VSCodeButton
          appearance="secondary"
          disabled={!isAnythingBeingEdited || editedMessage == null}
          onClick={() => {
            runOperation(
              new AmendMessageOperation(commit.hash, assertNonOptimistic(editedMessage)),
            );
            clearEditedCommitMessage(/* skip confirmation */ true);
          }}>
          <T>Amend Message</T>
        </VSCodeButton>
      )}

      {commit.isHead ? (
        <VSCodeButton
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
                title: t('Preferred Code Review command not yet configured'),
                message: (
                  <div>
                    <p>
                      <T replace={{$pr: <code>sl pr</code>, $ghstack: <code>sl ghstack</code>}}>
                        You can configure Sapling to use either $pr or $ghstack to submit for code
                        review on GitHub.
                      </T>
                    </p>
                    <p>
                      <T
                        replace={{
                          $config: <code>github.preferred_submit_command</code>,
                        }}>
                        Each submit command has tradeoffs, due to how GitHub creates Pull Requests.
                        This can be controlled by the $config config.
                      </T>
                    </p>
                    <p>
                      <T>To continue, select a command to use to submit.</T>
                    </p>
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
    <div className="commit-info-title-byline subtle">
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
    </div>
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

/**
 * Wrap `VSCodeTextArea` to auto-resize to minimum height and disallow newlines.
 * Like a `VSCodeTextField` that has text wrap inside.
 */
const MinHeightTextField = forwardRef(
  (
    props: React.ComponentProps<typeof VSCodeTextArea> & {
      onInput: (event: {target: {value: string}}) => unknown;
    },
    ref: ForwardedRef<typeof VSCodeTextArea>,
  ) => {
    const {onInput, ...rest} = props;

    // ref could also be a callback ref; don't bother supporting that right now.
    assert(typeof ref === 'object', 'MinHeightTextArea requires ref object');

    // whenever the value is changed, recompute & apply the minimum height
    useEffect(() => {
      const r = ref as MutableRefObject<typeof VSCodeTextArea>;
      const current = r?.current as unknown as HTMLInputElement;
      // height must be applied to textarea INSIDE shadowRoot of the VSCodeTextArea
      const innerTextArea = current?.shadowRoot?.querySelector('textarea');
      if (innerTextArea) {
        const resize = () => {
          innerTextArea.style.height = '';
          innerTextArea.style.height = `${innerTextArea.scrollHeight}px`;
        };
        resize();
        const obs = new ResizeObserver(resize);
        obs.observe(innerTextArea);
        return () => obs.unobserve(innerTextArea);
      }
    }, [props.value, ref]);

    return (
      <VSCodeTextArea
        ref={ref}
        {...rest}
        className={`min-height-text-area${rest.className ? ' ' + rest.className : ''}`}
        onInput={e => {
          const newValue = (e.target as HTMLInputElement)?.value
            // remove newlines so this acts like a textField rather than a textArea
            .replace(/(\r|\n)/g, '');
          onInput({target: {value: newValue}});
        }}
      />
    );
  },
);

function CommitInfoField({
  which,
  autoFocus,
  editedMessage,
  setEditedCommitMessage,
}: {
  which: keyof EditedMessage;
  autoFocus: boolean;
  editedMessage: EditedMessage;
  setEditedCommitMessage: SetterOrUpdater<EditedMessageUnlessOptimistic>;
}) {
  const ref = useRef(null);
  useEffect(() => {
    if (ref.current && autoFocus) {
      (ref.current as HTMLInputElement | null)?.focus();
    }
  }, [autoFocus, ref]);
  const Component = which === 'title' ? MinHeightTextField : VSCodeTextArea;
  const props =
    which === 'title'
      ? {}
      : {
          rows: 30,
          resize: 'vertical',
        };
  return (
    <Component
      ref={ref}
      {...props}
      value={editedMessage[which]}
      data-testid={`commit-info-${which}-field`}
      onInput={(event: FormEvent) => {
        setEditedCommitMessage({
          ...assertNonOptimistic(editedMessage),
          [which]: (event.target as HTMLInputElement)?.value,
        });
      }}
    />
  );
}
