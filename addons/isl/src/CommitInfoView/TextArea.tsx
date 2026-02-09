/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode, RefObject} from 'react';

import {TextArea} from 'isl-components/TextArea';
import {useEffect, useRef} from 'react';
import {InternalFieldName} from 'shared/constants';
import {
  FilePicker,
  ImageDropZone,
  PendingImageUploads,
  useUploadFilesCallback,
} from '../ImageUpload';
import {Internal} from '../Internal';
import {MinHeightTextField} from './MinHeightTextField';
import {convertFieldNameToKey} from './utils';

function moveCursorToEnd(element: HTMLTextAreaElement) {
  element.setSelectionRange(element.value.length, element.value.length);
}

export function CommitInfoTextArea({
  kind,
  name,
  autoFocus,
  editedMessage,
  setEditedField,
}: {
  kind: 'title' | 'textarea' | 'field';
  name: string;
  autoFocus: boolean;
  editedMessage: string;
  setEditedField: (fieldValue: string) => unknown;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (ref.current && autoFocus) {
      ref.current.focus();
      moveCursorToEnd(ref.current);
    }
  }, [autoFocus, ref]);
  const Component = kind === 'field' || kind === 'title' ? MinHeightTextField : TextArea;
  const props =
    kind === 'field' || kind === 'title'
      ? {}
      : ({
          rows: 15,
          resize: 'vertical',
        } as const);

  // The gh cli does not support uploading images for commit messages,
  // see https://github.com/cli/cli/issues/1895#issuecomment-718899617
  // for now, this is internal-only.
  const supportsImageUpload =
    kind === 'textarea' &&
    (Internal.supportsImageUpload === true ||
      // image upload is always enabled in tests
      process.env.NODE_ENV === 'test');

  const onInput = (event: {currentTarget: HTMLTextAreaElement}) => {
    setEditedField(event.currentTarget?.value);
  };

  const uploadFiles = useUploadFilesCallback(name, ref, onInput);

  const fieldKey = convertFieldNameToKey(name);

  const rendered = (
    <div className="commit-info-field">
      <Component
        ref={ref}
        {...props}
        onPaste={
          !supportsImageUpload
            ? undefined
            : (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
                if (event.clipboardData != null && event.clipboardData.files.length > 0) {
                  uploadFiles([...event.clipboardData.files]);
                  event.preventDefault();
                }
              }
        }
        value={editedMessage}
        data-testid={`commit-info-${fieldKey}-field`}
        onInput={onInput}
      />
      <EditorToolbar
        fieldName={name}
        uploadFiles={supportsImageUpload ? uploadFiles : undefined}
        textAreaRef={ref}
      />
    </div>
  );
  return !supportsImageUpload ? (
    rendered
  ) : (
    <ImageDropZone onDrop={uploadFiles}>{rendered}</ImageDropZone>
  );
}

/**
 * Floating button list at the bottom corner of the text area
 */
export function EditorToolbar({
  fieldName,
  textAreaRef,
  uploadFiles,
}: {
  fieldName: string;
  uploadFiles?: (files: Array<File>) => unknown;
  textAreaRef: RefObject<HTMLTextAreaElement>;
}) {
  const parts: Array<ReactNode> = [];
  if (uploadFiles != null) {
    parts.push(
      <PendingImageUploads fieldName={fieldName} key="pending-uploads" textAreaRef={textAreaRef} />,
    );
  }
  if (fieldName === InternalFieldName.TestPlan && Internal.RecommendTestPlanButton) {
    parts.push(<Internal.RecommendTestPlanButton key="recommend-test-plan" />);
  }
  if (fieldName === InternalFieldName.Summary && Internal.GenerateSummaryButton) {
    parts.push(<Internal.GenerateSummaryButton key="generate-summary" />);
  }
  if (uploadFiles != null) {
    parts.push(<FilePicker key="picker" uploadFiles={uploadFiles} />);
  }
  if (parts.length === 0) {
    return null;
  }
  return <div className="text-area-toolbar">{parts}</div>;
}
