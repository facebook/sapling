/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';
import type {CommitMessageFields} from './types';

import {globalRecoil} from '../AccessGlobalRecoil';
import serverAPI from '../ClientToServerAPI';
import {successionTracker} from '../SuccessionTracker';
import {latestCommitMessageFields} from '../codeReview/CodeReviewInfo';
import {readAtom, writeAtom} from '../jotaiUtils';
import {dagWithPreviews} from '../previews';
import {entangledAtoms} from '../recoilUtils';
import {selectedCommitInfos} from '../selection';
import {firstLine, registerCleanup, registerDisposable} from '../utils';
import {
  parseCommitMessageFields,
  allFieldsBeingEdited,
  anyEditsMade,
  applyEditedFields,
  commitMessageFieldsSchema,
} from './CommitMessageFields';
import {atom} from 'jotai';
import {atomFamily as jotaiAtomFamily} from 'jotai/utils';
import {atomFamily} from 'recoil';

export type EditedMessage = {fields: Partial<CommitMessageFields>};

export type CommitInfoMode = 'commit' | 'amend';

export const [commitMessageTemplate, commitMessageTemplateRecoil] = entangledAtoms<
  EditedMessage | undefined
>({
  key: 'commitMessageTemplate',
  default: undefined,
});
registerDisposable(
  commitMessageTemplate,
  serverAPI.onMessageOfType('fetchedCommitMessageTemplate', event => {
    const title = firstLine(event.template);
    const description = event.template.slice(title.length + 1);
    const schema = readAtom(commitMessageFieldsSchema);
    const fields = parseCommitMessageFields(schema, title, description);
    writeAtom(commitMessageTemplate, {fields});
  }),
  import.meta.hot,
);
registerCleanup(
  commitMessageTemplate,
  serverAPI.onSetup(() =>
    serverAPI.postMessage({
      type: 'fetchCommitMessageTemplate',
    }),
  ),
  import.meta.hot,
);

/** Typed update messages when submitting a commit or set of commits.
 * Unlike editedCommitMessages, you can't provide an update message when committing the first time,
 * so we don't need to track this state for 'head'.
 */
export const diffUpdateMessagesState = atomFamily<string, Hash>({
  key: 'diffUpdateMessagesState',
  default: '',
});

export const getDefaultEditedCommitMessage = (): EditedMessage => ({fields: {}});

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
 */
export const editedCommitMessages = jotaiAtomFamily((_hashOrHead: Hash | 'head') => {
  return atom<EditedMessage>(getDefaultEditedCommitMessage());
});

function updateEditedCommitMessagesFromSuccessions() {
  return successionTracker.onSuccessions(successions => {
    for (const [oldHash, newHash] of successions) {
      const existing = readAtom(editedCommitMessages(oldHash));
      writeAtom(editedCommitMessages(newHash), existing);

      const existingUpdateMessage = globalRecoil().getLoadable(diffUpdateMessagesState(oldHash));
      if (existingUpdateMessage.state === 'hasValue') {
        // TODO: this doesn't work if you have multiple commits selected...
        globalRecoil().set(diffUpdateMessagesState(newHash), existingUpdateMessage.valueOrThrow());
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

export const latestCommitMessageFieldsWithEdits = jotaiAtomFamily((hashOrHead: Hash | 'head') => {
  return atom(get => {
    const edited = get(editedCommitMessages(hashOrHead));
    const latest = get(latestCommitMessageFields(hashOrHead));
    return applyEditedFields(latest, edited.fields);
  });
});

/**
 * Fields being edited is computed from editedCommitMessage,
 * and reset to only substantially changed fields when changing commits.
 * This state skips the substantial changes check,
 * which allows all fields to be edited for example when clicking "amend...",
 * but without actually changing the underlying edited messages.
 */
export const forceNextCommitToEditAllFields = atom<boolean>(false);

export const unsavedFieldsBeingEdited = jotaiAtomFamily((hashOrHead: Hash | 'head') => {
  return atom(get => {
    const edited = get(editedCommitMessages(hashOrHead));
    const schema = get(commitMessageFieldsSchema);
    if (hashOrHead === 'head') {
      return allFieldsBeingEdited(schema);
    }
    return Object.fromEntries(schema.map(field => [field.key, field.key in edited.fields]));
  });
});

export const hasUnsavedEditedCommitMessage = jotaiAtomFamily((hashOrHead: Hash | 'head') => {
  return atom(get => {
    const beingEdited = get(unsavedFieldsBeingEdited(hashOrHead));
    if (Object.values(beingEdited).some(Boolean)) {
      // Some fields are being edited, let's look more closely to see if anything is actually different.
      const edited = get(editedCommitMessages(hashOrHead));
      const latest = get(latestCommitMessageFields(hashOrHead));
      const schema = get(commitMessageFieldsSchema);
      return anyEditsMade(schema, latest, edited.fields);
    }
    return false;
  });
});

export const commitMode = atom<CommitInfoMode>('amend');

export const commitInfoViewCurrentCommits = atom(get => {
  const selected = get(selectedCommitInfos);

  // show selected commit, if there's exactly 1
  const selectedCommit = selected.length === 1 ? selected[0] : undefined;
  const commit = selectedCommit ?? get(dagWithPreviews).resolve('.');

  if (commit == null) {
    return null;
  } else {
    return selected.length > 1 ? selected : [commit];
  }
});
