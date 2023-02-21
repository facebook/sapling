/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EditedMessage, EditedMessageUnlessOptimistic} from './CommitInfoState';
import type {ForwardedRef, MutableRefObject, ReactNode} from 'react';
import type {SetterOrUpdater} from 'recoil';

import {assertNonOptimistic} from './CommitInfoState';
import {
  useUploadFilesCallback,
  ImageDropZone,
  FilePicker,
  PendingImageUploads,
} from './ImageUpload';
import {Internal} from './Internal';
import {assert} from './utils';
import {VSCodeTextArea} from '@vscode/webview-ui-toolkit/react';
import {forwardRef, useRef, useEffect, type FormEvent} from 'react';

/**
 * VSCodeTextArea elements use custom components, which renders in a shadow DOM.
 * Most often, we want to access the inner <textarea>, which acts like a normal textarea.
 */
export function getInnerTextareaForVSCodeTextArea(
  outer: HTMLElement | null,
): HTMLTextAreaElement | null {
  return outer == null ? null : (outer as unknown as {control: HTMLTextAreaElement}).control;
}

/**
 * Wrap `VSCodeTextArea` to auto-resize to minimum height and disallow newlines.
 * Like a `VSCodeTextField` that has text wrap inside.
 */
const MinHeightTextField = forwardRef(
  (
    props: React.ComponentProps<typeof VSCodeTextArea> & {
      onInput: (event: {target: {value: string}}) => unknown;
    },
    ref: ForwardedRef<typeof VSCodeTextArea>,
  ) => {
    const {onInput, ...rest} = props;

    // ref could also be a callback ref; don't bother supporting that right now.
    assert(typeof ref === 'object', 'MinHeightTextArea requires ref object');

    // whenever the value is changed, recompute & apply the minimum height
    useEffect(() => {
      const r = ref as MutableRefObject<typeof VSCodeTextArea>;
      const current = r?.current as unknown as HTMLInputElement;
      // height must be applied to textarea INSIDE shadowRoot of the VSCodeTextArea
      const innerTextArea = current?.shadowRoot?.querySelector('textarea');
      if (innerTextArea) {
        const resize = () => {
          innerTextArea.style.height = '';
          innerTextArea.style.height = `${innerTextArea.scrollHeight}px`;
        };
        resize();
        const obs = new ResizeObserver(resize);
        obs.observe(innerTextArea);
        return () => obs.unobserve(innerTextArea);
      }
    }, [props.value, ref]);

    return (
      <VSCodeTextArea
        ref={ref}
        {...rest}
        className={`min-height-text-area${rest.className ? ' ' + rest.className : ''}`}
        onInput={e => {
          const newValue = (e.target as HTMLInputElement)?.value
            // remove newlines so this acts like a textField rather than a textArea
            .replace(/(\r|\n)/g, '');
          onInput({target: {value: newValue}});
        }}
      />
    );
  },
);

export function CommitInfoField({
  which,
  autoFocus,
  editedMessage,
  setEditedCommitMessage,
}: {
  which: keyof EditedMessage;
  autoFocus: boolean;
  editedMessage: EditedMessage;
  setEditedCommitMessage: SetterOrUpdater<EditedMessageUnlessOptimistic>;
}) {
  const ref = useRef(null);
  useEffect(() => {
    if (ref.current && autoFocus) {
      const inner = getInnerTextareaForVSCodeTextArea(ref.current as HTMLElement);
      inner?.focus();
    }
  }, [autoFocus, ref]);
  const Component = which === 'title' ? MinHeightTextField : VSCodeTextArea;
  const props =
    which === 'title'
      ? {}
      : {
          rows: 30,
          resize: 'vertical',
        };

  // The gh cli does not support uploading images for commit messages,
  // see https://github.com/cli/cli/issues/1895#issuecomment-718899617
  // for now, this is internal-only.
  const supportsImageUpload =
    which === 'description' &&
    (Internal.supportsImageUpload === true ||
      // image upload is always enabled in tests
      process.env.NODE_ENV === 'test');

  const uploadFiles = useUploadFilesCallback(ref);

  const rendered = (
    <div className="commit-info-field">
      <EditorToolbar
        uploadFiles={supportsImageUpload ? uploadFiles : undefined}
        textAreaRef={ref}
      />
      <Component
        ref={ref}
        {...props}
        onPaste={
          !supportsImageUpload
            ? null
            : (event: ClipboardEvent) => {
                if (event.clipboardData != null && event.clipboardData.files.length > 0) {
                  uploadFiles([...event.clipboardData.files]);
                }
              }
        }
        value={editedMessage[which]}
        data-testid={`commit-info-${which}-field`}
        onInput={(event: FormEvent) => {
          setEditedCommitMessage({
            ...assertNonOptimistic(editedMessage),
            [which]: (event.target as HTMLInputElement)?.value,
          });
        }}
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
  textAreaRef,
  uploadFiles,
}: {
  uploadFiles?: (files: Array<File>) => unknown;
  textAreaRef: MutableRefObject<unknown>;
}) {
  const parts: Array<ReactNode> = [];
  if (uploadFiles != null) {
    parts.push(<PendingImageUploads key="pending-uploads" textAreaRef={textAreaRef} />);
    parts.push(<FilePicker key="picker" uploadFiles={uploadFiles} />);
  }
  if (parts.length === 0) {
    return null;
  }
  return <div className="text-area-toolbar">{parts}</div>;
}
