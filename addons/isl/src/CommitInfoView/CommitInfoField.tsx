/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FieldConfig} from './types';
import type {ReactNode} from 'react';

import {Copyable} from '../Copyable';
import {T} from '../i18n';
import {RenderMarkup} from './RenderMarkup';
import {SeeMoreContainer} from './SeeMoreContainer';
import {CommitInfoTextArea} from './TextArea';
import {CommitInfoTextField} from './TextField';
import {extractTokens, TokensList} from './Tokens';
import {Section, SmallCapsTitle} from './utils';
import {Fragment} from 'react';
import {Icon} from 'shared/Icon';

export function CommitInfoField({
  field,
  isBeingEdited,
  readonly,
  content,
  editedField,
  startEditingField,
  setEditedField,
  extra,
  autofocus,
}: {
  field: FieldConfig;
  isBeingEdited: boolean;
  readonly: boolean;
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
              <T>{field.key}</T>
            </SmallCapsTitle>
            <CommitInfoTextArea
              kind={field.type}
              fieldConfig={field}
              name={field.key}
              autoFocus={autofocus ?? false}
              editedMessage={editedFieldContent}
              setEditedCommitMessage={setEditedField}
            />
          </Section>
        ) : (
          <ClickToEditField
            startEditingField={readonly ? undefined : startEditingField}
            kind={field.type}
            fieldKey={field.key}>
            <span>{content}</span>
            {readonly ? null : (
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
    const Wrapper = field.type === 'textarea' ? SeeMoreContainer : Fragment;
    if (field.type === 'read-only' && !content) {
      // don't render empty read-only fields, since you can't "click to edit"
      return null;
    }

    if (field.type !== 'read-only' && isBeingEdited) {
      return (
        <Section className="commit-info-field-section">
          <SmallCapsTitle>
            <Icon icon={field.icon} />
            {field.key}
          </SmallCapsTitle>
          {field.type === 'field' ? (
            <CommitInfoTextField
              name={field.key}
              autoFocus={autofocus ?? false}
              editedMessage={editedFieldContent}
              setEditedCommitMessage={setEditedField}
              typeaheadKind={field.typeaheadKind}
              maxTokens={field.maxTokens}
            />
          ) : (
            <CommitInfoTextArea
              kind={field.type}
              fieldConfig={field}
              name={field.key}
              autoFocus={autofocus ?? false}
              editedMessage={editedFieldContent}
              setEditedCommitMessage={setEditedField}
            />
          )}
          {extra}
        </Section>
      );
    }

    let renderedContent;
    if (content) {
      if (field.type === 'field') {
        const tokens = Array.isArray(content) ? content : extractTokens(content)[0];
        renderedContent = (
          <div className="commit-info-tokenized-field">
            <TokensList tokens={tokens} />
            {field.maxTokens === 1 && tokens.length > 0 && (
              <Copyable iconOnly>{tokens[0]}</Copyable>
            )}
          </div>
        );
      } else {
        if (Array.isArray(content) || !field.isRenderableMarkup) {
          renderedContent = content;
        } else {
          renderedContent = <RenderMarkup>{content}</RenderMarkup>;
        }
      }
    } else {
      renderedContent = (
        <span className="empty-description subtle">
          {readonly ? (
            <>
              <T replace={{$name: field.key}}> No $name</T>
            </>
          ) : (
            <>
              <Icon icon="add" />
              <T replace={{$name: field.key}}> Click to add $name</T>
            </>
          )}
        </span>
      );
    }

    return (
      <Section>
        <Wrapper>
          <ClickToEditField
            startEditingField={readonly ? undefined : startEditingField}
            kind={field.type}
            fieldKey={field.key}>
            <SmallCapsTitle>
              <Icon icon={field.icon} />
              <T>{field.key}</T>
              {readonly ? null : (
                <span className="hover-edit-button">
                  <Icon icon="edit" />
                </span>
              )}
            </SmallCapsTitle>
            {renderedContent}
          </ClickToEditField>
          {extra}
        </Wrapper>
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
  kind: 'title' | 'field' | 'textarea' | 'read-only';
}) {
  const editable = startEditingField != null && kind !== 'read-only';
  const renderKey = fieldKey.toLowerCase().replace(/\s/g, '-');
  return (
    <div
      className={`commit-info-rendered-${kind}${editable ? '' : ' non-editable'}`}
      data-testid={`commit-info-rendered-${renderKey}`}
      onClick={
        startEditingField != null && kind !== 'read-only'
          ? () => {
              startEditingField();
            }
          : undefined
      }
      onKeyPress={
        startEditingField != null && kind !== 'read-only'
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
