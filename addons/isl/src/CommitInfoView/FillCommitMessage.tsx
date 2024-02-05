/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';
import type {CommitInfoMode} from './CommitInfoState';
import type {CommitMessageFields, FieldConfig} from './types';

import {globalRecoil} from '../AccessGlobalRecoil';
import {LinkButton} from '../components/LinkButton';
import {T, t} from '../i18n';
import {dagWithPreviews} from '../previews';
import {layout} from '../stylexUtils';
import {font, spacing} from '../tokens.stylex';
import {useModal} from '../useModal';
import {commitMessageTemplate, editedCommitMessages} from './CommitInfoState';
import {
  parseCommitMessageFields,
  commitMessageFieldsSchema,
  mergeCommitMessageFields,
  findConflictingFieldsWhenMerging,
} from './CommitMessageFields';
import {SmallCapsTitle} from './utils';
import * as stylex from '@stylexjs/stylex';
import {useRecoilCallback} from 'recoil';
import {Icon} from 'shared/Icon';

const fillCommitMessageMethods: Array<{
  label: string;
  getMessage: (
    commit: CommitInfo,
    mode: CommitInfoMode,
  ) => CommitMessageFields | undefined | Promise<CommitMessageFields | undefined>;
}> = [
  {
    label: t('last commit'),
    getMessage: (commit: CommitInfo, mode: CommitInfoMode) => {
      const schema = globalRecoil().getLoadable(commitMessageFieldsSchema).valueMaybe();
      const dag = globalRecoil().getLoadable(dagWithPreviews).valueMaybe();
      if (!dag || !schema) {
        return undefined;
      }
      // If in commit mode, "last commit" is actually the head commit.
      // Otherwise, it's the parent.
      const parent = dag.get(mode === 'commit' ? commit.hash : commit.parents[0]);
      if (!parent) {
        return undefined;
      }
      return parseCommitMessageFields(schema, parent.title, parent.description);
    },
  },
  {
    label: t('template file'),
    getMessage: (_commit: CommitInfo, _mode: CommitInfoMode) => {
      const template = globalRecoil().getLoadable(commitMessageTemplate).valueMaybe();
      return template?.fields as CommitMessageFields | undefined;
    },
  },
];

export function FillCommitMessage({commit, mode}: {commit: CommitInfo; mode: CommitInfoMode}) {
  const showModal = useModal();
  const fillMessage = useRecoilCallback(
    ({set, snapshot}) =>
      async (newMessage: CommitMessageFields) => {
        const hashOrHead = mode === 'commit' ? 'head' : commit.hash;
        // TODO: support amending a message

        const schema = snapshot.getLoadable(commitMessageFieldsSchema).valueMaybe();
        const existing = snapshot.getLoadable(editedCommitMessages(hashOrHead)).valueMaybe();
        if (existing?.type === 'optimistic' || schema == null) {
          return;
        }
        if (existing == null) {
          set(editedCommitMessages(hashOrHead), {fields: newMessage});
          return;
        }
        const oldMessage = existing.fields as CommitMessageFields;
        const buttons = [
          {label: t('Cancel')},
          {label: t('Overwrite')},
          {label: t('Merge'), primary: true},
        ] as const;
        let answer: (typeof buttons)[number] | undefined = buttons[2]; // merge if no conflicts
        const conflictingFields = findConflictingFieldsWhenMerging(schema, oldMessage, newMessage);
        if (conflictingFields.length > 0) {
          answer = await showModal({
            type: 'confirm',
            title: t('Commit Messages Conflict'),
            icon: 'warning',
            message: (
              <MessageConflictWarning
                conflictingFields={conflictingFields}
                oldMessage={oldMessage}
                newMessage={newMessage}
              />
            ),
            buttons,
          });
        }
        if (answer === buttons[2]) {
          // TODO: T177275949 should we warn about conflicts instead of just merging?
          const merged = mergeCommitMessageFields(schema, oldMessage, newMessage);
          set(editedCommitMessages(hashOrHead), {fields: merged});
          return;
        } else if (answer === buttons[1]) {
          set(editedCommitMessages(hashOrHead), {fields: newMessage});
          return;
        }
      },
  );

  const methods = (
    <>
      {fillCommitMessageMethods.map(method => (
        <LinkButton
          key={method.label}
          onClick={async () => {
            const newMessage = await method.getMessage(commit, mode);
            if (newMessage == null) {
              return;
            }
            fillMessage(newMessage);
          }}>
          {method.label}
        </LinkButton>
      ))}
    </>
  );
  return (
    <div {...stylex.props(layout.flexRow, styles.container)}>
      <T replace={{$methods: methods}}>Fill commit message from $methods</T>
    </div>
  );
}

function MessageConflictWarning({
  conflictingFields,
  oldMessage,
  newMessage,
}: {
  conflictingFields: Array<FieldConfig>;
  oldMessage: CommitMessageFields;
  newMessage: CommitMessageFields;
}) {
  return (
    <div data-testid="fill-message-conflict-warning">
      <div>
        <T>The new commit message being loaded conflicts with your current message.</T>
      </div>
      <div>
        <T>Would you like to merge them or overwrite your current message with the new one?</T>
      </div>
      <div style={{marginBlock: spacing.pad}}>
        <T>These fields are conflicting:</T>
      </div>
      <div>
        {conflictingFields.map((field, i) => (
          <div key={i} {...stylex.props(layout.paddingBlock)}>
            <SmallCapsTitle>
              <Icon icon={field.icon} />
              {field.key}
            </SmallCapsTitle>
            <div {...stylex.props(layout.flexRow)}>
              <b>
                <T>Current:</T>
              </b>
              <Truncate>{oldMessage[field.key]}</Truncate>
            </div>
            <div {...stylex.props(layout.flexRow)}>
              <b>
                <T>New:</T>
              </b>
              <Truncate>{newMessage[field.key]}</Truncate>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function Truncate({children}: {children: string | Array<string>}) {
  const content = Array.isArray(children) ? children.join(', ') : children;
  return (
    <span {...stylex.props(styles.truncate)} title={content}>
      {content}
    </span>
  );
}

const styles = stylex.create({
  container: {
    padding: spacing.half,
    paddingInline: spacing.pad,
    gap: spacing.half,
    alignItems: 'baseline',
    fontSize: font.small,
    marginInline: spacing.pad,
    marginTop: spacing.half,
  },
  truncate: {
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    maxWidth: 500,
  },
});
