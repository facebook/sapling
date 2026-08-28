/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Hash} from '../types';
import type {CommitMessageFields} from './types';

import {ErrorNotice} from 'isl-components/ErrorNotice';
import {atom} from 'jotai';
import {InternalFieldName} from 'shared/constants';
import {firstLine, splitCommitMessage} from 'shared/utils';
import serverAPI from '../ClientToServerAPI';
import {successionTracker} from '../SuccessionTracker';
import {tracker} from '../analytics';
import {effectiveSchemaForCommit, latestCommitMessageFields} from '../codeReview/CodeReviewInfo';
import {expandCommitInfoView} from '../drawerState';
import {atomFamilyWeak, localStorageBackedAtomFamily, readAtom, writeAtom} from '../jotaiUtils';
import {AmendMessageOperation} from '../operations/AmendMessageOperation';
import {AmendOperation, PartialAmendOperation} from '../operations/AmendOperation';
import {CommitOperation, PartialCommitOperation} from '../operations/CommitOperation';
import {onOperationExited, queuedOperations, queuedOperationsErrorAtom} from '../operationsState';
import {dagWithPreviews} from '../previews';
import {selectedCommitInfos, selectedCommits} from '../selection';
import {latestHeadCommit} from '../serverAPIState';
import {showToast} from '../toast';
import {registerCleanup, registerDisposable} from '../utils';
import {
  allFieldsBeingEdited,
  anyEditsMade,
  applyEditedFields,
  commitMessageFieldsSchema,
  mergeCommitMessageFields,
  mergeOnlyEmptyMessageFields,
  parseCommitMessageFields,
} from './CommitMessageFields';

export type EditedMessage = Partial<CommitMessageFields>;

export type CommitInfoMode = 'commit' | 'amend';

export function getCommitFieldAIRequestKey(
  target: Hash | 'head',
  field: InternalFieldName,
): string {
  return `${target}:${field}`;
}

export const commitFieldAIInFlight = atom(new Set<string>());
registerCleanup(
  commitFieldAIInFlight,
  serverAPI.onSetup(() => writeAtom(commitFieldAIInFlight, new Set<string>())),
  import.meta.hot,
);

export const commitMessageTemplate = atom<EditedMessage | undefined>(undefined);
registerDisposable(
  commitMessageTemplate,
  serverAPI.onMessageOfType('fetchedCommitMessageTemplate', event => {
    const title = firstLine(event.template);
    const description = event.template.slice(title.length + 1);
    const schema = readAtom(commitMessageFieldsSchema);
    const fields = parseCommitMessageFields(schema, title, description);
    writeAtom(commitMessageTemplate, fields);
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
export const diffUpdateMessagesState = atomFamilyWeak((_hash: Hash) => atom<string>(''));

export const getDefaultEditedCommitMessage = (): EditedMessage => ({});

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
export const editedCommitMessages = localStorageBackedAtomFamily<Hash | 'head', EditedMessage>(
  'isl.edited-commit-messages:',
  () => getDefaultEditedCommitMessage(),
);

function updateEditedCommitMessagesFromSuccessions() {
  return successionTracker.onSuccessions(successions => {
    for (const [oldHash, newHash] of successions) {
      const existing = readAtom(editedCommitMessages(oldHash));
      writeAtom(
        editedCommitMessages(newHash),
        Object.keys(existing).length === 0
          ? // If the edited message is empty, write undefined to remove from persisted storage
            undefined
          : existing,
      );

      const existingUpdateMessage = readAtom(diffUpdateMessagesState(oldHash));
      if (existingUpdateMessage && existingUpdateMessage !== '') {
        // TODO: this doesn't work if you have multiple commits selected...
        writeAtom(diffUpdateMessagesState(newHash), existingUpdateMessage);
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
registerCleanup(successionTracker, updateEditedCommitMessagesFromSuccessions, import.meta.hot);

// Handle updateDraftCommitMessage messages
registerDisposable(
  serverAPI,
  serverAPI.onMessageOfType('updateDraftCommitMessage', event => {
    const title = event.title;
    const description = event.description;
    const mode = event.mode ?? 'commit'; // Default to 'commit' if not specified
    const hash = event.hash ?? readAtom(latestHeadCommit)?.hash ?? 'head';
    // If a specific hash is provided, ensure the commit exists and select it
    const commit = readAtom(dagWithPreviews).get(hash);
    if (mode !== 'commit' && commit?.phase === 'public') {
      showToast(
        <ErrorNotice
          title="Cannot update commit message"
          error={
            new Error(
              `Commit ${hash} is a public commit and cannot be modified. Please select a draft commit or use commit mode instead.`,
            )
          }
        />,
        {durationMs: 8000},
      );
      return;
    }

    const schema = readAtom(commitMessageFieldsSchema);
    const fields = parseCommitMessageFields(schema, title, description);

    const key = mode === 'commit' ? 'head' : hash;
    const currentMessage = readAtom(editedCommitMessages(key));
    // Non-empty fields replace existing values, empty fields preserve current values.
    // By passing fields first, mergeOnlyEmptyMessageFields will prefer the new fields when non-empty.
    writeAtom(
      editedCommitMessages(key),
      mergeOnlyEmptyMessageFields(schema, fields, currentMessage as CommitMessageFields),
    );

    if (event.preserveFocus !== true) {
      expandCommitInfoView();
      writeAtom(selectedCommits, new Set([hash]));
      writeAtom(rawCommitMode, mode);
    }
  }),
);

registerDisposable(
  serverAPI,
  serverAPI.onMessageOfType('platform/commitFieldAIStatus', event => {
    const requestKey = getCommitFieldAIRequestKey(event.target, event.field);
    writeAtom(commitFieldAIInFlight, current => {
      if (event.status === 'loading') {
        return new Set(current).add(requestKey);
      }
      if (!current.has(requestKey)) {
        return current;
      }
      const next = new Set(current);
      next.delete(requestKey);
      return next;
    });

    if (event.status === 'error') {
      showToast(
        <ErrorNotice
          title={
            event.field === InternalFieldName.Summary
              ? 'Failed to generate summary'
              : 'Failed to recommend test plan'
          }
          error={new Error(event.error ?? 'Unknown error')}
        />,
        {
          durationMs: 5000,
          key: `commit-field-ai-error-${event.field}`,
        },
      );
    }
  }),
);

registerDisposable(
  serverAPI,
  onOperationExited((progress, exitedOperation) => {
    if (progress.exitCode === 0) {
      return;
    }

    const queuedError = readAtom(queuedOperationsErrorAtom);
    const queued = readAtom(queuedOperations);
    const affectedOperations =
      queuedError?.operationThatErrored?.id !== exitedOperation.id
        ? [exitedOperation, ...queued]
        : [exitedOperation, ...queuedError.operations];

    for (const operation of affectedOperations) {
      const isCommit =
        operation instanceof CommitOperation || operation instanceof PartialCommitOperation;
      const isAmend =
        operation instanceof AmendOperation || operation instanceof PartialAmendOperation;
      const isMetaedit = operation instanceof AmendMessageOperation;
      if (!(isCommit || isAmend || isMetaedit)) {
        continue;
      }

      // Operation involving commit message failed, let's restore your edited commit message so you might save it or try again
      const message = operation.message;
      if (message == null) {
        continue;
      }

      const headOrHash = isCommit
        ? 'head'
        : isMetaedit
          ? operation.getCommitHash()
          : readAtom(latestHeadCommit)?.hash;

      if (!headOrHash) {
        continue;
      }

      const [title, description] = splitCommitMessage(message);

      tracker.track('RecoverCommitMessageFromOperationError');

      const schema = readAtom(commitMessageFieldsSchema);
      const fields = parseCommitMessageFields(schema, title, description);
      const currentMessage = readAtom(editedCommitMessages(headOrHash));
      writeAtom(
        editedCommitMessages(headOrHash),
        mergeCommitMessageFields(schema, currentMessage as CommitMessageFields, fields),
      );
      writeAtom(commitMode, isCommit ? 'commit' : 'amend');
      if (!isCommit) {
        writeAtom(selectedCommits, new Set([headOrHash]));
      }
    }
  }),
  import.meta.hot,
);

export const latestCommitMessageFieldsWithEdits = atomFamilyWeak((hashOrHead: Hash | 'head') => {
  return atom(get => {
    const edited = get(editedCommitMessages(hashOrHead));
    const latest = get(latestCommitMessageFields(hashOrHead));
    return applyEditedFields(latest, edited);
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

export const unsavedFieldsBeingEdited = atomFamilyWeak((hashOrHead: Hash | 'head') => {
  return atom(get => {
    const edited = get(editedCommitMessages(hashOrHead));
    const schema = get(effectiveSchemaForCommit(hashOrHead));
    if (hashOrHead === 'head') {
      return allFieldsBeingEdited(schema);
    }
    return Object.fromEntries(schema.map(field => [field.key, field.key in edited]));
  });
});

export const hasUnsavedEditedCommitMessage = atomFamilyWeak((hashOrHead: Hash | 'head') => {
  return atom(get => {
    const beingEdited = get(unsavedFieldsBeingEdited(hashOrHead));
    if (Object.values(beingEdited).some(Boolean)) {
      // Some fields are being edited, let's look more closely to see if anything is actually different.
      const edited = get(editedCommitMessages(hashOrHead));
      const latest = get(latestCommitMessageFields(hashOrHead));
      const schema = get(effectiveSchemaForCommit(hashOrHead));
      return anyEditsMade(schema, latest, edited);
    }
    return false;
  });
});

/**
 * Toggle state between commit/amend modes. Note that this may be "commit" even if
 * the commit info is not looking at the head commit (this allows persistence as you select other commits and come back).
 * We should only behave in "commit" mode when in commit mode AND looking at the head commit.
 * Prefer using `commitMode` atom.
 */
const rawCommitMode = atom<CommitInfoMode>('amend');

/**
 * Whether the commit info view is in "commit" or "amend" mode.
 * It may only be in the "commit" mode when the commit being viewed is the head commit,
 * though it may be set to "commit" mode even when looking at a non-head commit,
 * and it'll be in commit when when you do look at the head commit.
 */
export const commitMode = atom(
  get => {
    const commitInfoCommit = get(commitInfoViewCurrentCommits);
    const rawMode = get(rawCommitMode);
    if (commitInfoCommit == null) {
      // loading state
      return 'amend';
    }
    if (commitInfoCommit.length === 1 && commitInfoCommit[0].isDot) {
      // allow using "commit" mode only if looking at exactly the single head commit
      return rawMode;
    }
    // otherwise, it's a non-head commit or multi-selection, so only show "amend" mode
    return 'amend';
  },
  (_get, set, newMode: CommitInfoMode | ((m: CommitInfoMode) => CommitInfoMode)) => {
    set(rawCommitMode, newMode);
  },
);

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

/**
 * Derived atom that reactively computes the parent commit context and parsed fields.
 * Returns undefined if there is no eligible parent commit.
 */
export const parentCommitContextAtom = atom(get => {
  const currentCommits = get(commitInfoViewCurrentCommits);
  const mode = get(commitMode);
  const commit = currentCommits?.length === 1 ? currentCommits[0] : undefined;
  if (!commit) {
    return undefined;
  }
  const isCommitMode = mode === 'commit';
  const parentCommit = get(dagWithPreviews).get(isCommitMode ? commit.hash : commit.parents[0]);
  if (!parentCommit || parentCommit.phase === 'public') {
    return undefined;
  }
  const parentSchema = get(effectiveSchemaForCommit(parentCommit.hash));
  const parentFields = parseCommitMessageFields(
    parentSchema,
    parentCommit.title,
    parentCommit.description,
  );
  return {commit, isCommitMode, parentFields};
});

/**
 * Copy a field's value from the parent commit into the current commit's edited message.
 */
export function copyFromParentCommit(fieldKey: string): void {
  const ctx = readAtom(parentCommitContextAtom);
  if (!ctx) {
    return;
  }
  const {commit, isCommitMode, parentFields} = ctx;
  const parentVal = parentFields[fieldKey];
  if (!parentVal) {
    return;
  }

  tracker.track('CopyCommitFieldsFromParent');
  const hashOrHead = isCommitMode ? 'head' : commit.hash;
  const schema = readAtom(effectiveSchemaForCommit(hashOrHead));
  const field = schema.find(f => f.key === fieldKey);
  const val = Array.isArray(parentVal) ? parentVal.join(',') : parentVal;
  const newVal = field?.type === 'field' ? val + ',' : val;
  writeAtom(editedCommitMessages(hashOrHead), prev => ({
    ...prev,
    [fieldKey]: field?.type === 'field' ? newVal.split(',') : newVal,
  }));
}
