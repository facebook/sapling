/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitInfo} from '../types';
import type {CommitInfoMode} from './CommitInfoState';
import type {CommitMessageFields, FieldConfig} from './types';

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {LinkButton} from 'isl-components/LinkButton';
import {DOCUMENTATION_DELAY, Tooltip} from 'isl-components/Tooltip';
import {useCallback} from 'react';
import {useContextMenu} from 'shared/ContextMenu';
import {font, spacing} from '../../../components/theme/tokens.stylex';
import {FlexSpacer} from '../ComponentUtils';
import {Internal} from '../Internal';
import {tracker} from '../analytics';
import {useFeatureFlagSync} from '../featureFlags';
import {T, t} from '../i18n';
import {readAtom, writeAtom} from '../jotaiUtils';
import platform from '../platform';
import {dagWithPreviews} from '../previews';
import {layout} from '../stylexUtils';
import {useModal} from '../useModal';
import {
  commitMessageTemplate,
  editedCommitMessages,
  getDefaultEditedCommitMessage,
} from './CommitInfoState';
import {
  commitMessageFieldsSchema,
  findConflictingFieldsWhenMerging,
  mergeCommitMessageFields,
  mergeOnlyEmptyMessageFields,
  parseCommitMessageFields,
} from './CommitMessageFields';
import {SmallCapsTitle} from './utils';

/**
 * The last entry in a tokenized field value is used as the value being typed in the editor.
 * When filling, we want all the values to be tokens and not inserted to the editors.
 * Add empty entries at the end of all tokenized fields to force tokens.
 */
function forceTokenizeAllFields(fields: CommitMessageFields): CommitMessageFields {
  const result: CommitMessageFields = {};
  for (const [key, value] of Object.entries(fields)) {
    if (Array.isArray(value)) {
      result[key] = value.length > 0 && value.at(-1) ? [...value, ''] : value;
    } else {
      result[key] = value;
    }
  }
  return result;
}

const fillCommitMessageMethods: Array<{
  key: string;
  label: string;
  getMessage: (
    commit: CommitInfo,
    mode: CommitInfoMode,
  ) => CommitMessageFields | undefined | Promise<CommitMessageFields | undefined>;
  tooltip: string;
}> = [
  {
    key: 'last-commit',
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
      return forceTokenizeAllFields(fields);
    },
  },
  {
    key: 'template-file',
    label: t('template file'),
    tooltip: t(
      'Fill in your configured commit message template.\nSee `sl help config` for more information.',
    ),
    getMessage: (_commit: CommitInfo, _mode: CommitInfoMode) => {
      const template = readAtom(commitMessageTemplate);
      return template as CommitMessageFields | undefined;
    },
  },
  ...(Internal.fillCommitMessageMethods ?? []),
];

export function FillCommitMessage({commit, mode}: {commit: CommitInfo; mode: CommitInfoMode}) {
  const showModal = useModal();
  const menu = useContextMenu(() => [
    {
      label: t('Clear commit message'),
      onClick: async () => {
        const confirmed = await platform.confirm(
          t('Are you sure you want to clear the currently edited commit message?'),
        );
        if (confirmed) {
          writeAtom(editedCommitMessages('head'), {});
        }
      },
    },
  ]);
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
        {label: t('Merge')},
        {label: t('Only Fill Empty'), primary: true},
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
      if (answer === buttons[3]) {
        const merged = mergeOnlyEmptyMessageFields(schema, oldMessage, newMessage);
        writeAtom(editedCommitMessages(hashOrHead), merged);
        return;
      } else if (answer === buttons[2]) {
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

  const showDevmate =
    useFeatureFlagSync(Internal.featureFlags?.DevmateGenerateCommitMessage) &&
    platform.platformName === 'vscode';

  const methods = (
    <>
      {fillCommitMessageMethods
        // Only show Devmate option if allowlisted in GK
        .filter(method => method.key !== 'devmate' || showDevmate)
        .map(method => (
          <Tooltip
            title={method.tooltip}
            key={method.key}
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
      <FlexSpacer />
      <Button icon onClick={menu} data-testid="fill-commit-message-more-options">
        <Icon icon="ellipsis" />
      </Button>
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
      <div style={{maxWidth: '500px'}}>
        <T>
          Would you like to overwrite your current message with the new one, merge them, or only
          fill fields that are empty in the current message?
        </T>
      </div>
      <div style={{marginBlock: spacing.pad}}>
        <T>These fields are conflicting:</T>
      </div>
      <div>
        {conflictingFields.map((field, i) => {
          const oldValue = oldMessage[field.key];
          const newValue = newMessage[field.key];
          return (
            <div key={i} {...stylex.props(layout.paddingBlock)}>
              <SmallCapsTitle>
                <Icon icon={field.icon} />
                {field.key}
              </SmallCapsTitle>
              <div {...stylex.props(layout.flexRow)}>
                <b>
                  <T>Current:</T>
                </b>
                <Truncate>{oldValue}</Truncate>
              </div>
              <div {...stylex.props(layout.flexRow)}>
                <b>
                  <T>New:</T>
                </b>
                <Truncate>{newValue}</Truncate>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Truncate({children}: {children: string | Array<string>}) {
  const content = Array.isArray(children)
    ? children.filter(v => v.trim() !== '').join(', ')
    : children;
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
    paddingBottom: 0,
    gap: spacing.half,
    alignItems: 'center',
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
