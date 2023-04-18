/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ForwardedRef, MutableRefObject, ReactNode} from 'react';

import {
  useUploadFilesCallback,
  ImageDropZone,
  FilePicker,
  PendingImageUploads,
} from '../ImageUpload';
import {Internal} from '../Internal';
import {assert} from '../utils';
import {getInnerTextareaForVSCodeTextArea} from './utils';
import {VSCodeTextArea} from '@vscode/webview-ui-toolkit/react';
import {forwardRef, useRef, useEffect} from 'react';

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

export function CommitInfoTextArea({
  kind,
  name,
  autoFocus,
  editedMessage,
  setEditedCommitMessage,
}: {
  kind: 'title' | 'textarea' | 'field';
  name: string;
  autoFocus: boolean;
  editedMessage: string;
  setEditedCommitMessage: (fieldValue: string) => unknown;
}) {
  const ref = useRef(null);
  useEffect(() => {
    if (ref.current && autoFocus) {
      const inner = getInnerTextareaForVSCodeTextArea(ref.current as HTMLElement);
      inner?.focus();
    }
  }, [autoFocus, ref]);
  const Component = kind === 'field' || kind === 'title' ? MinHeightTextField : VSCodeTextArea;
  const props =
    kind === 'field' || kind === 'title'
      ? {}
      : {
          rows: 15,
          resize: 'vertical',
        };

  // The gh cli does not support uploading images for commit messages,
  // see https://github.com/cli/cli/issues/1895#issuecomment-718899617
  // for now, this is internal-only.
  const supportsImageUpload =
    kind === 'textarea' &&
    (Internal.supportsImageUpload === true ||
      // image upload is always enabled in tests
      process.env.NODE_ENV === 'test');

  const onInput = (event: {target: HTMLInputElement}) => {
    setEditedCommitMessage((event.target as HTMLInputElement)?.value);
  };

  const uploadFiles = useUploadFilesCallback(ref, onInput);

  const fieldKey = name.toLowerCase().replace(/\s/g, '-');

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
                  event.preventDefault();
                }
              }
        }
        value={editedMessage}
        data-testid={`commit-info-${fieldKey}-field`}
        onInput={onInput}
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
