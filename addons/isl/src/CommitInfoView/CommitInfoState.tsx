/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';
import type {CommitMessageFields, FieldsBeingEdited} from './types';

import serverAPI from '../ClientToServerAPI';
import {latestCommitTreeMap} from '../serverAPIState';
import {firstLine} from '../utils';
import {
  commitMessageFieldsSchema,
  parseCommitMessageFields,
  findFieldsBeingEdited,
  emptyCommitMessageFields,
} from './CommitMessageFields';
import {atomFamily, selectorFamily, atom} from 'recoil';

export type EditedMessage = {fields: CommitMessageFields};

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
      serverAPI.onConnectOrReconnect(() =>
        serverAPI.postMessage({
          type: 'fetchCommitMessageTemplate',
        }),
      ),
  ],
});

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
export const editedCommitMessages = atomFamily<EditedMessageUnlessOptimistic, Hash | 'head'>({
  key: 'editedCommitMessages',
  default: selectorFamily({
    key: 'editedCommitMessages/defaults',
    get:
      hash =>
      ({get}) => {
        if (hash === 'head') {
          const template = get(commitMessageTemplate);
          return template ?? {fields: emptyCommitMessageFields(get(commitMessageFieldsSchema))};
        }
        // TODO: is there a better way we should derive `isOptimistic`
        // from `get(treeWithPreviews)`, rather than using non-previewed map?
        const map = get(latestCommitTreeMap);
        const info = map.get(hash)?.info;
        if (info == null) {
          return {type: 'optimistic'};
        }
        const fields = parseCommitMessageFields(
          get(commitMessageFieldsSchema),
          info.title,
          info.description,
        );
        return {fields};
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
        return Object.values(edited).some(Boolean);
      }
      // TODO: T149536695 use treeWithPreviews so this indicator is accurate on top of previews
      const original = get(latestCommitTreeMap).get(hash)?.info;
      const schema = get(commitMessageFieldsSchema);
      const parsed = parseCommitMessageFields(
        schema,
        original?.title ?? '',
        original?.description ?? '',
      );
      return Object.values(findFieldsBeingEdited(schema, edited.fields, parsed)).some(Boolean);
    },
});

export const commitFieldsBeingEdited = atom<FieldsBeingEdited>({
  key: 'commitFieldsBeingEdited',
  default: {}, // empty object is valid as FieldsBeingEdited, and constructable without the schema
});

export const commitMode = atom<CommitInfoMode>({
  key: 'commitMode',
  default: 'amend',
});
