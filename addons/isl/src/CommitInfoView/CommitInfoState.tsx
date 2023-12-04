/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo, Hash} from '../types';
import type {CommitMessageFields, FieldsBeingEdited} from './types';

import {globalRecoil} from '../AccessGlobalRecoil';
import serverAPI from '../ClientToServerAPI';
import {successionTracker} from '../SuccessionTracker';
import {latestCommitMessageFields} from '../codeReview/CodeReviewInfo';
import {dagWithPreviews} from '../previews';
import {selectedCommitInfos} from '../selection';
import {firstLine} from '../utils';
import {
  commitMessageFieldsSchema,
  parseCommitMessageFields,
  allFieldsBeingEdited,
  noFieldsBeingEdited,
  anyEditsMade,
  applyEditedFields,
} from './CommitMessageFields';
import {atomFamily, selectorFamily, atom, selector} from 'recoil';

export type EditedMessage = {fields: Partial<CommitMessageFields>};

export type CommitInfoMode = 'commit' | 'amend';
export type EditedMessageUnlessOptimistic =
  | (EditedMessage & {type?: undefined})
  | {type: 'optimistic'; fields?: CommitMessageFields};

/**
 * Throw if the edited message is of optimistic type.
 * We expect:
 *  - editedCommitMessage('head') should never be optimistic
 *  - editedCommitMessage(hashForCommitInTheTree) should not be optimistic
 *  - editedCommitMessage(hashForCommitNotInTheTree) should be optimistic
 */
export function assertNonOptimistic(editedMessage: EditedMessageUnlessOptimistic): EditedMessage {
  if (editedMessage.type === 'optimistic') {
    throw new Error('Expected edited message to not be for optimistic commit');
  }
  return editedMessage;
}

export const commitMessageTemplate = atom<EditedMessage | undefined>({
  key: 'commitMessageTemplate',
  default: undefined,
  effects: [
    ({setSelf, getLoadable}) => {
      const disposable = serverAPI.onMessageOfType('fetchedCommitMessageTemplate', event => {
        const title = firstLine(event.template);
        const description = event.template.slice(title.length + 1);
        const schema = getLoadable(commitMessageFieldsSchema).valueOrThrow();
        const fields = parseCommitMessageFields(schema, title, description);
        setSelf({fields});
      });
      return () => disposable.dispose();
    },
    () =>
      serverAPI.onSetup(() =>
        serverAPI.postMessage({
          type: 'fetchCommitMessageTemplate',
        }),
      ),
  ],
});

/** Typed update messages when submitting a commit or set of commits.
 * Unlike editedCommitMessages, you can't provide an update message when committing the first time,
 * so we don't need to track this state for 'head'.
 */
export const diffUpdateMessagesState = atomFamily<string, Hash>({
  key: 'diffUpdateMessagesState',
  default: '',
});

/**
 * Map of hash -> latest edited commit message, representing any changes made to the commit's message fields.
 * Only fields that are edited are entered here. Fields that are not edited are not in the object.
 *
 * `{}` corresponds to the original commit message.
 * `{Title: 'hello'}` means the title was changed to "hello", but all other fields are unchanged.
 *
 * When you begin editing a field, that field must be initialized in the EditedMessage with the latest value.
 * This also stores the state of new commit messages being written, keyed by "head" instead of a commit hash.
 * Note: this state should be cleared when amending / committing / meta-editing.
 *
 * TODO: This state has a separate field for if it's optimistic, but this is no longer really needed. Remove this.
 */
export const editedCommitMessages = atomFamily<EditedMessageUnlessOptimistic, Hash | 'head'>({
  key: 'editedCommitMessages',
  default: () => ({fields: {}}),
});

function updateEditedCommitMessagesFromSuccessions() {
  return successionTracker.onSuccessions(successions => {
    for (const [oldHash, newHash] of successions) {
      const existing = globalRecoil().getLoadable(editedCommitMessages(oldHash));
      if (
        existing.state === 'hasValue' &&
        // Never copy an "optimistic" message during succession, we have no way to clear it out.
        // "optimistic" may also correspond to a message which was not edited,
        // for which the hash no longer exists in the tree.
        // We should just use the atom's default, which lets it populate correctly.
        existing.valueOrThrow().type !== 'optimistic'
      ) {
        globalRecoil().set(editedCommitMessages(newHash), existing.valueOrThrow());
      }

      const existingUpdateMessage = globalRecoil().getLoadable(diffUpdateMessagesState(oldHash));
      if (existingUpdateMessage.state === 'hasValue') {
        // TODO: this doesn't work if you have multiple commits selected...
        globalRecoil().set(diffUpdateMessagesState(oldHash), existingUpdateMessage.valueOrThrow());
      }
    }
  });
}
let editedCommitMessageSuccessionDisposable = updateEditedCommitMessagesFromSuccessions();
export const __TEST__ = {
  renewEditedCommitMessageSuccessionSubscription() {
    editedCommitMessageSuccessionDisposable();
    editedCommitMessageSuccessionDisposable = updateEditedCommitMessagesFromSuccessions();
  },
};

export const latestCommitMessageFieldsWithEdits = selectorFamily<
  CommitMessageFields,
  Hash | 'head'
>({
  key: 'latestCommitMessageFieldsWithEdits',
  get:
    hash =>
    ({get}) => {
      const edited = get(editedCommitMessages(hash));
      const latest = get(latestCommitMessageFields(hash));
      if (edited.type === 'optimistic') {
        return latest;
      }
      return applyEditedFields(latest, edited.fields);
    },
});

/**
 * Fields being edited is computed from editedCommitMessage,
 * and reset to only substantially changed fields when changing commits.
 * This state skips the substantial changes check,
 * which allows all fields to be edited for example when clicking "amend...",
 * but without actually changing the underlying edited messages.
 */
export const forceNextCommitToEditAllFields = atom({
  key: 'forceNextCommitToEditAllFields',
  default: false,
});

export const unsavedFieldsBeingEdited = selectorFamily<FieldsBeingEdited, Hash | 'head'>({
  key: 'unsavedFieldsBeingEdited',
  get:
    hash =>
    ({get}) => {
      const edited = get(editedCommitMessages(hash));
      const schema = get(commitMessageFieldsSchema);
      if (edited.type === 'optimistic') {
        return noFieldsBeingEdited(schema);
      }
      if (hash === 'head') {
        return allFieldsBeingEdited(schema);
      }
      return Object.fromEntries(schema.map(field => [field.key, field.key in edited.fields]));
    },
});

export const hasUnsavedEditedCommitMessage = selectorFamily<boolean, Hash | 'head'>({
  key: 'hasUnsavedEditedCommitMessage',
  get:
    hash =>
    ({get}) => {
      const beingEdited = get(unsavedFieldsBeingEdited(hash));
      if (Object.values(beingEdited).some(Boolean)) {
        // Some fields are being edited, let's look more closely to see if anything is actually different.
        const edited = get(editedCommitMessages(hash));
        if (edited.type === 'optimistic') {
          return false;
        }
        const latest = get(latestCommitMessageFields(hash));
        const schema = get(commitMessageFieldsSchema);
        return anyEditsMade(schema, latest, assertNonOptimistic(edited).fields);
      }
      return false;
    },
});

export const commitMode = atom<CommitInfoMode>({
  key: 'commitMode',
  default: 'amend',
});

export const commitInfoViewCurrentCommits = selector<Array<CommitInfo> | null>({
  key: 'commitInfoViewCurrentCommits',
  get: ({get}) => {
    const selected = get(selectedCommitInfos);

    // show selected commit, if there's exactly 1
    const selectedCommit = selected.length === 1 ? selected[0] : undefined;
    const commit = selectedCommit ?? get(dagWithPreviews).resolve('.');

    if (commit == null) {
      return null;
    } else {
      return selected.length > 1 ? selected : [commit];
    }
  },
});
