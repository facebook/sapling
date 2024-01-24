/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import clientToServerAPI from './ClientToServerAPI';
import {getInnerTextareaForVSCodeTextArea} from './CommitInfoView/utils';
import {InlineErrorBadge} from './ErrorNotice';
import {Tooltip} from './Tooltip';
import {T, t} from './i18n';
import platform from './platform';
import {replaceInTextArea, insertAtCursor} from './textareaUtils';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {type MutableRefObject, useState, type ReactNode, useId} from 'react';
import {atom, selector, useRecoilCallback, useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';
import {randomId} from 'shared/utils';

export type ImageUploadStatus = {id: number} & (
  | {status: 'pending'}
  | {status: 'complete'}
  | {status: 'error'; error: Error; resolved: boolean}
  | {status: 'canceled'}
);
export const imageUploadState = atom<{next: number; states: Record<number, ImageUploadStatus>}>({
  key: 'imageUploadState',
  default: {next: 1, states: {}},
});
export const numPendingImageUploads = selector({
  key: 'numPendingImageUploads',
  get: ({get}): number => {
    const state = get(imageUploadState);
    return Object.values(state.states).filter(state => state.status === 'pending').length;
  },
});
type UnresolvedErrorImageUploadStatus = ImageUploadStatus & {status: 'error'; resolved: false};
export const unresolvedErroredImagedUploads = selector({
  key: 'unresolvedErroredImagedUploads',
  get: ({get}): Array<UnresolvedErrorImageUploadStatus> => {
    const state = get(imageUploadState);
    return Object.values(state.states).filter(
      (state): state is UnresolvedErrorImageUploadStatus =>
        state.status === 'error' && !state.resolved,
    );
  },
});

function placeholderForImageUpload(id: number): string {
  // TODO: it might be better to use a `contenteditable: true` div rather than
  // inserting text as a placeholder. It's possible to partly or completely delete this
  // text and then the image link won't get inserted properly.
  // TODO: We could add a text-based spinner that we periodically update, to make it feel like it's in progress:
  // ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏
  return `【 Uploading #${id} 】`;
}

/**
 * Send a File's contents to the server to get uploaded by an upload service,
 * and return the link to embed in the textArea.
 */
export async function uploadFile(file: File): Promise<string> {
  const payload = await file.arrayBuffer();
  const id = randomId();
  clientToServerAPI.postMessageWithPayload({type: 'uploadFile', filename: file.name, id}, payload);
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
export function PendingImageUploads({textAreaRef}: {textAreaRef: MutableRefObject<unknown>}) {
  const numPending = useRecoilValue(numPendingImageUploads);
  const unresolvedErrors = useRecoilValue(unresolvedErroredImagedUploads);
  const [isHovering, setIsHovering] = useState(false);
  const onCancel = useRecoilCallback(({set}) => () => {
    setIsHovering(false);
    // Canceling ongoing uploads doesn't actualy interrupt the async work for the uploads,
    // it just deletes the tracking state, by replacing 'pending' uploads as 'cancelled'.
    set(imageUploadState, current => {
      const canceledIds: Array<number> = [];
      // TODO: This cancels ALL ongoing uploads, even from other text areas, if any exist.
      // imageUploadStates should contain a unique id per text area,
      // so we can cancel just this text area's uploads
      const newState = {
        ...current,
        states: Object.fromEntries(
          Object.entries(current.states).map(([idStr, state]) => {
            const id = Number(idStr);
            if (state.status === 'pending') {
              canceledIds.push(id);
              return [id, {state: 'cancelled', id}];
            }
            return [id, state];
          }),
        ) as Record<number, ImageUploadStatus>,
      };

      const textArea = getInnerTextareaForVSCodeTextArea(textAreaRef.current as HTMLElement | null);
      if (textArea) {
        for (const id of canceledIds) {
          const placeholder = placeholderForImageUpload(id);
          replaceInTextArea(textArea, placeholder, ''); // delete placeholder
        }
      }
      return newState;
    });
  });

  const onDismissErrors = useRecoilCallback(({set}) => () => {
    set(imageUploadState, value => ({
      ...value,
      states: Object.fromEntries(
        Object.entries(value.states).map(([id, state]) => [
          id,
          state.status === 'error' ? {...state, resolved: true} : state,
        ]),
      ),
    }));
  });

  if (unresolvedErrors.length === 0 && numPending === 0) {
    return null;
  }

  let content;
  if (unresolvedErrors.length > 0) {
    content = (
      <span className="upload-status-error">
        <Tooltip title={t('Click to dismiss error')}>
          <VSCodeButton
            appearance="icon"
            onClick={onDismissErrors}
            data-testid="dismiss-upload-errors">
            <Icon icon="close" />
          </VSCodeButton>
        </Tooltip>
        <InlineErrorBadge error={unresolvedErrors[0].error} title={<T>Image upload failed</T>}>
          <T count={unresolvedErrors.length}>imageUploadFailed</T>
        </InlineErrorBadge>
      </span>
    );
  } else if (numPending > 0) {
    if (isHovering) {
      content = (
        <VSCodeButton appearance="icon">
          <Icon icon="stop-circle" slot="start" />
          <T>Click to cancel</T>
        </VSCodeButton>
      );
    } else {
      content = (
        <VSCodeButton appearance="icon">
          <Icon icon="loading" slot="start" />
          <T count={numPending}>numImagesUploading</T>
        </VSCodeButton>
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
          <VSCodeButton
            appearance="icon"
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
              }
            }}>
            <PaperclipIcon />
          </VSCodeButton>
        </Tooltip>
      </label>
    </span>
  );
}

export function useUploadFilesCallback(
  ref: MutableRefObject<unknown>,
  onInput: (event: {target: HTMLInputElement}) => unknown,
) {
  return useRecoilCallback(({snapshot, set}) => async (files: Array<File>) => {
    // capture snapshot of next before doing async work
    // we need to account for all files in this batch
    let {next} = snapshot.getLoadable(imageUploadState).valueOrThrow();

    const textArea = getInnerTextareaForVSCodeTextArea(ref.current as HTMLElement);
    if (textArea != null) {
      // manipulating the text area directly does not emit change events,
      // we need to simulate those ourselves so that controlled text areas
      // update their underlying store
      const emitChangeEvent = () => {
        onInput({
          target: textArea as unknown as HTMLInputElement,
        });
      };

      await Promise.all(
        files.map(async file => {
          const id = next;
          next += 1;
          const state = {status: 'pending' as const, id};
          set(imageUploadState, v => ({next, states: {...v.states, [id]: state}}));
          // insert pending text
          const placeholder = placeholderForImageUpload(state.id);
          insertAtCursor(textArea, placeholder);
          emitChangeEvent();

          // start the file upload
          try {
            const uploadedFileText = await uploadFile(file);
            set(imageUploadState, v => ({
              next,
              states: {...v.states, [id]: {status: 'complete' as const, id}},
            }));
            replaceInTextArea(textArea, placeholder, uploadedFileText);
            emitChangeEvent();
          } catch (error) {
            set(imageUploadState, v => ({
              next,
              states: {
                ...v.states,
                [id]: {status: 'error' as const, id, error: error as Error, resolved: false},
              },
            }));
            replaceInTextArea(textArea, placeholder, ''); // delete placeholder
            emitChangeEvent();
          }
        }),
      );
    }
  });
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
 * Can you belive codicons don't have a paperclip icon?
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
