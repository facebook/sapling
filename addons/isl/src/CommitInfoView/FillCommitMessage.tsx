/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';
import type {CommitInfoMode} from './CommitInfoState';
import type {CommitMessageFields, FieldConfig} from './types';

import {Internal} from '../Internal';
import {DOCUMENTATION_DELAY, Tooltip} from '../Tooltip';
import {tracker} from '../analytics';
import {LinkButton} from '../components/LinkButton';
import {T, t} from '../i18n';
import {readAtom, writeAtom} from '../jotaiUtils';
import {dagWithPreviews} from '../previews';
import {layout} from '../stylexUtils';
import {font, spacing} from '../tokens.stylex';
import {useModal} from '../useModal';
import {
  getDefaultEditedCommitMessage,
  commitMessageTemplate,
  editedCommitMessages,
} from './CommitInfoState';
import {
  parseCommitMessageFields,
  commitMessageFieldsSchema,
  mergeCommitMessageFields,
  findConflictingFieldsWhenMerging,
} from './CommitMessageFields';
import {SmallCapsTitle} from './utils';
import * as stylex from '@stylexjs/stylex';
import {useCallback} from 'react';
import {Icon} from 'shared/Icon';

const fillCommitMessageMethods: Array<{
  label: string;
  getMessage: (
    commit: CommitInfo,
    mode: CommitInfoMode,
  ) => CommitMessageFields | undefined | Promise<CommitMessageFields | undefined>;
  tooltip: string;
}> = [
  {
    label: t('last commit'),
    tooltip: t("Fill in the previous commit's message here."),
    getMessage: (commit: CommitInfo, mode: CommitInfoMode) => {
      const schema = readAtom(commitMessageFieldsSchema);
      const dag = readAtom(dagWithPreviews);
      if (!dag || !schema) {
        return undefined;
      }
      // If in commit mode, "last commit" is actually the head commit.
      // Otherwise, it's the parent.
      const parent = dag.get(mode === 'commit' ? commit.hash : commit.parents[0]);
      if (!parent) {
        return undefined;
      }
      const fields = parseCommitMessageFields(schema, parent.title, parent.description);
      if (Internal.diffFieldTag) {
        // don't fill in diff field, so we don't conflict with a previous diff
        delete fields[Internal.diffFieldTag];
      }
      return fields;
    },
  },
  {
    label: t('template file'),
    tooltip: t(
      'Fill in your configured commit message template.\nSee `sl help config` for more information.',
    ),
    getMessage: (_commit: CommitInfo, _mode: CommitInfoMode) => {
      const template = readAtom(commitMessageTemplate);
      return template as CommitMessageFields | undefined;
    },
  },
];

export function FillCommitMessage({commit, mode}: {commit: CommitInfo; mode: CommitInfoMode}) {
  const showModal = useModal();
  const fillMessage = useCallback(
    async (newMessage: CommitMessageFields) => {
      const hashOrHead = mode === 'commit' ? 'head' : commit.hash;
      // TODO: support amending a message

      const schema = readAtom(commitMessageFieldsSchema);
      const existing = readAtom(editedCommitMessages(hashOrHead));
      if (schema == null) {
        return;
      }
      if (existing == null) {
        writeAtom(editedCommitMessages(hashOrHead), getDefaultEditedCommitMessage());
        return;
      }
      const oldMessage = existing as CommitMessageFields;
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
        writeAtom(editedCommitMessages(hashOrHead), merged);
        return;
      } else if (answer === buttons[1]) {
        writeAtom(editedCommitMessages(hashOrHead), newMessage);
        return;
      }
    },
    [commit, mode, showModal],
  );

  const methods = (
    <>
      {fillCommitMessageMethods.map(method => (
        <Tooltip
          title={method.tooltip}
          key={method.label}
          placement="bottom"
          delayMs={DOCUMENTATION_DELAY}>
          <LinkButton
            onClick={() => {
              tracker.operation(
                'FillCommitMessage',
                'FetchError',
                {extras: {method: method.label}},
                async () => {
                  const newMessage = await method.getMessage(commit, mode);
                  if (newMessage == null) {
                    return;
                  }
                  fillMessage(newMessage);
                },
              );
            }}>
            {method.label}
          </LinkButton>
        </Tooltip>
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
