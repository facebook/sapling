/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {type MutableRefObject, useState, type ReactNode} from 'react';
import {atom, useRecoilCallback} from 'recoil';

export type ImageUploadStatus = {id: number} & (
  | {status: 'pending'}
  | {status: 'complete'}
  | {status: 'error'}
  | {status: 'canceled'}
);
export const imageUploadState = atom<{next: number; states: Record<number, ImageUploadStatus>}>({
  key: 'imageUploadState',
  default: {next: 1, states: {}},
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
 * Upload a file to the given file upload service,
 * and return the link to embed in the textArea.
 */
export async function uploadFile(file: File): Promise<string> {
  await new Promise(res => setTimeout(res, 30_000)); // temporary testing

  return file.name;
}

export function useUploadFilesCallback(ref: MutableRefObject<unknown>) {
  return useRecoilCallback(({snapshot, set}) => async (files: Array<File>) => {
    let {next} = snapshot.getLoadable(imageUploadState).valueOrThrow();

    const textArea =
      ref.current == null ? null : (ref.current as {control: HTMLInputElement}).control;
    if (textArea) {
      // capture snapshot of next before doing async work
      // we need to account for all files in this batch

      await Promise.all(
        files.map(async file => {
          const id = next;
          next += 1;
          const state = {status: 'pending' as const, id};
          set(imageUploadState, v => ({next, states: {...v.states, [id]: state}}));
          // insert pending text
          const placeholder = placeholderForImageUpload(state.id);
          insertAtCursor(textArea, placeholder);

          // start the file upload
          try {
            const uploadedFileText = await uploadFile(file);
            set(imageUploadState, v => ({
              next,
              states: {...v.states, [id]: {status: 'complete' as const, id}},
            }));
            replaceInTextArea(textArea, placeholder, uploadedFileText);
          } catch (err) {
            set(imageUploadState, v => ({
              next,
              states: {...v.states, [id]: {status: 'error' as const, id}},
            }));
            replaceInTextArea(textArea, placeholder, ''); // delete placeholder
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
 * Replace text in a text area.
 * Re-adjust cursor & selection to be the same as before the replacement.
 */
function replaceInTextArea(textArea: HTMLInputElement, oldText: string, newText: string) {
  const {selectionStart, selectionEnd} = textArea;
  const insertionSpot = textArea.value.indexOf(oldText);
  textArea.value = textArea.value.replace(oldText, newText);
  // re-select whatever we had selected before.
  // if the new text is longer, we may need to add additional length
  if (selectionStart) {
    textArea.selectionStart =
      selectionStart > insertionSpot
        ? selectionStart + (newText.length - oldText.length)
        : selectionStart;
  }
  if (selectionEnd) {
    textArea.selectionEnd =
      selectionEnd > insertionSpot
        ? selectionEnd + (newText.length - oldText.length)
        : selectionEnd;
  }
}

/**
 * Insert text into a text area at the cursor location.
 * Add spaces before/after as necessary so the new text does not neighbor the existing content.
 * If text is selected, replace the selected text with the new text.
 * If nothing is selected, append to the end.
 */
function insertAtCursor(textArea: HTMLInputElement, text: string) {
  if (textArea.selectionStart != null) {
    const startPos = textArea.selectionStart;
    const endPos = textArea.selectionEnd;
    const nextCharPos = endPos ?? startPos;
    const previousChar = textArea.value[startPos - 1];
    const nextChar = textArea.value[nextCharPos];
    const isWhitespace = (s: string | undefined) => !s || /[ \n\t]/.test(s);
    // if inserting next to whitespace, no need to add more.
    // if inserting next to text, add a space before to avoid the link becoming invalid.
    const prefix = isWhitespace(previousChar) ? '' : ' ';
    // similarly for suffix
    const suffix = isWhitespace(nextChar) ? '' : ' ';
    const totalAddedLength = text.length + prefix.length + suffix.length;
    textArea.value =
      textArea.value.substring(0, startPos) +
      prefix +
      text +
      suffix +
      (endPos != null ? textArea.value.substring(endPos, textArea.value.length) : '');
    const newPos = startPos + totalAddedLength;
    textArea.selectionStart = newPos;
    textArea.selectionEnd = newPos;
  } else {
    // no selection => append to the end
    const prefix = /\s/.test(textArea.value[textArea.value.length - 1]) ? '' : ' ';
    textArea.value += prefix + text;
  }
}
