/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RefObject} from 'react';

import {Button} from 'isl-components/Button';
import {InlineErrorBadge} from 'isl-components/ErrorNotice';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtomValue} from 'jotai';
import {useId, useState, type ReactNode} from 'react';
import {randomId} from 'shared/utils';
import clientToServerAPI from './ClientToServerAPI';
import {T, t} from './i18n';
import {atomFamilyWeak, readAtom, writeAtom} from './jotaiUtils';
import platform from './platform';
import {insertAtCursor, replaceInTextArea} from './textareaUtils';

export type ImageUploadStatus = {id: number; field: string} & (
  | {status: 'pending'}
  | {status: 'complete'}
  | {status: 'error'; error: Error; resolved: boolean}
  | {status: 'canceled'}
);
export const imageUploadState = atom<{next: number; states: Record<number, ImageUploadStatus>}>({
  next: 1,
  states: {},
});

/**
 * Number of currently ongoing image uploads for a given field name.
 * If undefined is givens as the field name, all pending uploads across all fields are counted. */
export const numPendingImageUploads = atomFamilyWeak((fieldName: string | undefined) =>
  atom((get): number => {
    const state = get(imageUploadState);
    return Object.values(state.states).filter(
      state => (fieldName == null || state.field === fieldName) && state.status === 'pending',
    ).length;
  }),
);

type UnresolvedErrorImageUploadStatus = ImageUploadStatus & {status: 'error'; resolved: false};
export const unresolvedErroredImagedUploads = atomFamilyWeak((fieldName: string) =>
  atom(get => {
    const state = get(imageUploadState);
    return Object.values(state.states).filter(
      (state): state is UnresolvedErrorImageUploadStatus =>
        state.field == fieldName && state.status === 'error' && !state.resolved,
    );
  }),
);

function placeholderForImageUpload(id: number): string {
  // TODO: it might be better to use a `contenteditable: true` div rather than
  // inserting text as a placeholder. It's possible to partly or completely delete this
  // text and then the image link won't get inserted properly.
  // TODO: We could add a text-based spinner that we periodically update, to make it feel like it's in progress:
  // ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏
  return `【 Uploading #${id} 】`;
}

function getBase64(file: File): Promise<string> {
  return new Promise((res, rej) => {
    const reader = new FileReader();
    reader.onload = function () {
      const result = reader.result as string | null;
      if (result == null) {
        rej(new Error('got empty file content'));
        return;
      }
      // The loaded image will be in the form:
      // data:image/png;base64,iVB0R...
      // Strip away the prefix (up to the ',') to get to the actual base64 portion
      const commaIndex = result.indexOf(',');
      if (commaIndex === -1) {
        rej(new Error('file data is not in `data:*/*;base64,` format'));
        return;
      }
      res(result.slice(commaIndex + 1));
    };
    reader.onerror = function (error) {
      rej(error);
    };
    reader.readAsDataURL(file);
  });
}

/**
 * Send a File's contents to the server to get uploaded by an upload service,
 * and return the link to embed in the textArea.
 */
export async function uploadFile(file: File): Promise<string> {
  const base64 = await getBase64(file);
  const id = randomId();
  clientToServerAPI.postMessage({
    type: 'uploadFile',
    filename: file.name,
    id,
    b64Content: base64,
  });
  const result = await clientToServerAPI.nextMessageMatching(
    'uploadFileResult',
    message => message.id === id,
  );

  if (result.result.error) {
    throw result.result.error;
  }

  const uploadedUrl = result.result.value;
  return uploadedUrl;
}

/**
 * Summary of ongoing image uploads. Click to cancel all ongoing uploads.
 */
export function PendingImageUploads({
  fieldName,
  textAreaRef,
}: {
  fieldName: string;
  textAreaRef: RefObject<HTMLTextAreaElement>;
}) {
  const numPending = useAtomValue(numPendingImageUploads(fieldName));
  const unresolvedErrors = useAtomValue(unresolvedErroredImagedUploads(fieldName));
  const [isHovering, setIsHovering] = useState(false);
  const onCancel = () => {
    setIsHovering(false);
    // Canceling ongoing uploads doesn't actually interrupt the async work for the uploads,
    // it just deletes the tracking state, by replacing 'pending' uploads as 'cancelled'.
    writeAtom(imageUploadState, current => {
      const canceledIds: Array<number> = [];
      const newState = {
        ...current,
        states: Object.fromEntries(
          Object.entries(current.states).map(([idStr, state]) => {
            const id = Number(idStr);
            if (state.field === fieldName && state.status === 'pending') {
              canceledIds.push(id);
              return [id, {state: 'cancelled', id, fieldName}];
            }
            return [id, state];
          }),
        ) as Record<number, ImageUploadStatus>,
      };

      const textArea = textAreaRef.current;
      if (textArea) {
        for (const id of canceledIds) {
          const placeholder = placeholderForImageUpload(id);
          replaceInTextArea(textArea, placeholder, ''); // delete placeholder
        }
      }
      return newState;
    });
  };

  const onDismissErrors = () => {
    writeAtom(imageUploadState, value => ({
      ...value,
      states: Object.fromEntries(
        Object.entries(value.states).map(([id, state]) => [
          id,
          state.field === fieldName && state.status === 'error'
            ? {...state, resolved: true}
            : state,
        ]),
      ),
    }));
  };

  if (unresolvedErrors.length === 0 && numPending === 0) {
    return null;
  }

  let content;
  if (unresolvedErrors.length > 0) {
    content = (
      <span className="upload-status-error">
        <Tooltip title={t('Click to dismiss error')}>
          <Button icon onClick={onDismissErrors} data-testid="dismiss-upload-errors">
            <Icon icon="close" />
          </Button>
        </Tooltip>
        <InlineErrorBadge error={unresolvedErrors[0].error} title={<T>Image upload failed</T>}>
          <T count={unresolvedErrors.length}>imageUploadFailed</T>
        </InlineErrorBadge>
      </span>
    );
  } else if (numPending > 0) {
    if (isHovering) {
      content = (
        <Button icon>
          <Icon icon="stop-circle" slot="start" />
          <T>Click to cancel</T>
        </Button>
      );
    } else {
      content = (
        <Button icon>
          <Icon icon="loading" slot="start" />
          <T count={numPending}>numImagesUploading</T>
        </Button>
      );
    }
  }

  return (
    <span
      className="upload-status"
      onClick={onCancel}
      onMouseEnter={() => setIsHovering(true)}
      onMouseLeave={() => setIsHovering(false)}>
      {content}
    </span>
  );
}

export function FilePicker({uploadFiles}: {uploadFiles: (files: Array<File>) => unknown}) {
  const id = useId();
  return (
    <span key="choose-file">
      <input
        type="file"
        accept="image/*,video/*"
        className="choose-file"
        data-testid="attach-file-input"
        id={id}
        multiple
        onChange={event => {
          if (event.target.files) {
            uploadFiles([...event.target.files]);
          }
          event.target.files = null;
        }}
      />
      <label htmlFor={id}>
        <Tooltip
          title={t(
            'Choose image or video files to upload. Drag & Drop and Pasting images or videos is also supported.',
          )}>
          <Button
            icon
            data-testid="attach-file-button"
            onClick={e => {
              if (platform.chooseFile != null) {
                e.preventDefault();
                e.stopPropagation();
                platform.chooseFile('Choose file to upload', /* multi */ true).then(chosen => {
                  if (chosen.length > 0) {
                    uploadFiles(chosen);
                  }
                });
              } else {
                // By default, <button> clicks do not forward to the parent <label>'s htmlFor target.
                // Manually trigger a click on the element instead.
                const input = document.getElementById(id);
                if (input) {
                  input.click();
                }
              }
            }}>
            <PaperclipIcon />
          </Button>
        </Tooltip>
      </label>
    </span>
  );
}

export function useUploadFilesCallback(
  fieldName: string,
  ref: RefObject<HTMLTextAreaElement>,
  onInput: (e: {currentTarget: HTMLTextAreaElement}) => unknown,
) {
  return async (files: Array<File>) => {
    // capture snapshot of next before doing async work
    // we need to account for all files in this batch
    let {next} = readAtom(imageUploadState);

    const textArea = ref.current;
    if (textArea != null) {
      // manipulating the text area directly does not emit change events,
      // we need to simulate those ourselves so that controlled text areas
      // update their underlying store
      const emitChangeEvent = () => {
        onInput({
          currentTarget: textArea,
        });
      };

      await Promise.all(
        files.map(async file => {
          const id = next;
          next += 1;
          const state: ImageUploadStatus = {status: 'pending' as const, id, field: fieldName};
          writeAtom(imageUploadState, v => ({next, states: {...v.states, [id]: state}}));
          // insert pending text
          const placeholder = placeholderForImageUpload(state.id);
          insertAtCursor(textArea, placeholder);
          emitChangeEvent();

          // start the file upload
          try {
            const uploadedFileText = await uploadFile(file);
            writeAtom(imageUploadState, v => ({
              next,
              states: {...v.states, [id]: {status: 'complete' as const, id, field: fieldName}},
            }));
            replaceInTextArea(textArea, placeholder, uploadedFileText);
            emitChangeEvent();
          } catch (error) {
            writeAtom(imageUploadState, v => ({
              next,
              states: {
                ...v.states,
                [id]: {
                  status: 'error' as const,
                  id,
                  field: fieldName,
                  error: error as Error,
                  resolved: false,
                },
              },
            }));
            replaceInTextArea(textArea, placeholder, ''); // delete placeholder
            emitChangeEvent();
          }
        }),
      );
    }
  };
}

/**
 * Wrapper around children to allow dragging & dropping a file onto it.
 * Renders a highlight when hovering with a file.
 */
export function ImageDropZone({
  children,
  onDrop,
}: {
  children: ReactNode;
  onDrop: (files: Array<File>) => void;
}) {
  const [isHoveringToDropImage, setIsHoveringToDrop] = useState(false);
  const highlight = (e: React.DragEvent) => {
    if (e.dataTransfer.files.length > 0 || e.dataTransfer.items.length > 0) {
      // only highlight if you're dragging files
      setIsHoveringToDrop(true);
    }
    e.preventDefault();
    e.stopPropagation();
  };
  const unhighlight = (e: React.DragEvent) => {
    setIsHoveringToDrop(false);
    e.preventDefault();
    e.stopPropagation();
  };
  return (
    <div
      className={'image-drop-zone' + (isHoveringToDropImage ? ' hovering-to-drop' : '')}
      onDragEnter={highlight}
      onDragOver={highlight}
      onDragLeave={unhighlight}
      onDrop={event => {
        unhighlight(event);
        onDrop([...event.dataTransfer.files]);
      }}>
      {children}
    </div>
  );
}

/**
 * Codicon-like 16x16 paperclip icon.
 * This seems to be the standard iconographic way to attach files to a text area.
 * Can you believe codicons don't have a paperclip icon?
 */
function PaperclipIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
      <path
        d="M5.795 9.25053L8.43233 6.58103C8.72536 6.35421 9.44589 6.03666 9.9837 6.58103C10.5215 7.1254 10.2078 7.78492 9.9837 8.04664L5.49998 12.5C4.99998 13 3.91267 13.2914 3.00253 12.2864C2.0924 11.2814 2.49999 10 3.00253 9.4599L8.89774 3.64982C9.51829 3.12638 11.111 2.42499 12.5176 3.80685C13.9242 5.1887 13.5 7 12.5 8L8.43233 12.2864"
        stroke="currentColor"
        strokeWidth="0.8"
        strokeLinecap="round"
      />
    </svg>
  );
}
