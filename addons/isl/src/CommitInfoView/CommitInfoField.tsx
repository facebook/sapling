/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitMessageFields} from './CommitMessageFields';
import type {FieldConfig} from './types';
import type {ReactNode} from 'react';

import {T} from '../i18n';
import {CommitInfoTextArea} from './TextArea';
import {Section, SmallCapsTitle} from './utils';
import {Icon} from 'shared/Icon';

export function CommitInfoField({
  field,
  isBeingEdited,
  isOptimistic,
  content,
  editedField,
  startEditingField,
  setEditedField,
  extra,
  autofocus,
}: {
  field: FieldConfig<CommitMessageFields>;
  isBeingEdited: boolean;
  isOptimistic: boolean;
  startEditingField: () => void;
  content?: string | Array<string>;
  editedField: string | Array<string> | undefined;
  setEditedField: (value: string) => unknown;
  extra?: JSX.Element;
  autofocus?: boolean;
}): JSX.Element | null {
  const editedFieldContent =
    editedField == null ? '' : Array.isArray(editedField) ? editedField.join(', ') : editedField;
  if (field.type === 'title') {
    return (
      <>
        {isBeingEdited ? (
          <Section className="commit-info-title-field-section">
            <SmallCapsTitle>
              <Icon icon="milestone" />
              <T>{field.name}</T>
            </SmallCapsTitle>
            <CommitInfoTextArea
              kind={field.type}
              name={field.key}
              autoFocus={autofocus ?? false}
              editedMessage={editedFieldContent}
              setEditedCommitMessage={setEditedField}
            />
          </Section>
        ) : (
          <ClickToEditField
            startEditingField={isOptimistic ? undefined : startEditingField}
            kind={field.type}
            fieldKey={field.key}>
            <span>{content}</span>
            {isOptimistic ? null : (
              <span className="hover-edit-button">
                <Icon icon="edit" />
              </span>
            )}
          </ClickToEditField>
        )}
        {extra}
      </>
    );
  } else {
    return isBeingEdited ? (
      <Section className="commit-info-field-section">
        <SmallCapsTitle>
          <Icon icon={field.icon} />
          {field.name}
        </SmallCapsTitle>
        <CommitInfoTextArea
          kind={field.type}
          name={field.key}
          autoFocus={autofocus ?? false}
          editedMessage={editedFieldContent}
          setEditedCommitMessage={setEditedField}
        />
      </Section>
    ) : (
      <Section>
        <ClickToEditField
          startEditingField={isOptimistic ? undefined : startEditingField}
          kind={field.type}
          fieldKey={field.key}>
          <SmallCapsTitle>
            <Icon icon={field.icon} />
            <T>{field.name}</T>
            <span className="hover-edit-button">
              <Icon icon="edit" />
            </span>
          </SmallCapsTitle>
          {content ? (
            <div>{content}</div>
          ) : (
            <span className="empty-description subtle">
              {isOptimistic ? (
                <>
                  <T>No description</T>
                  <T replace={{$name: field.name}}> No $name</T>
                </>
              ) : (
                <>
                  <Icon icon="add" />
                  <T replace={{$name: field.name}}> Click to add $name</T>
                </>
              )}
            </span>
          )}
        </ClickToEditField>
      </Section>
    );
  }
}

function ClickToEditField({
  children,
  startEditingField,
  fieldKey,
  kind,
}: {
  children: ReactNode;
  /** function to run when you click to edit. If null, the entire field will be non-editable. */
  startEditingField?: () => void;
  fieldKey: string;
  kind: 'title' | 'field' | 'textarea';
}) {
  const editable = startEditingField != null;
  return (
    <div
      className={`commit-info-rendered-${kind}${editable ? '' : ' non-editable'}`}
      data-testid={`commit-info-rendered-${fieldKey}`}
      onClick={
        startEditingField != null
          ? () => {
              startEditingField();
            }
          : undefined
      }
      onKeyPress={
        startEditingField != null
          ? e => {
              if (e.key === 'Enter') {
                startEditingField();
              }
            }
          : undefined
      }
      tabIndex={0}>
      {children}
    </div>
  );
}
