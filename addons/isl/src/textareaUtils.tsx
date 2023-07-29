/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Replace text in a text area.
 * Re-adjust cursor & selection to be the same as before the replacement.
 */
export function replaceInTextArea(textArea: HTMLTextAreaElement, oldText: string, newText: string) {
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
export function insertAtCursor(textArea: HTMLTextAreaElement, text: string) {
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
