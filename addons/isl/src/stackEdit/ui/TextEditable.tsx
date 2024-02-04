/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import deepEqual from 'fast-deep-equal';
import React, {useEffect, useLayoutEffect, useRef, useState} from 'react';

import './TextEditable.css';

/** Text selection range. Unit: characters. */
export type RangeInfo = {
  start: number;
  end: number;
};

/**
 * The `index` suggests that the `RangeInfo` comes form `rangeInfos[index]`,
 * and we expect the corresponding DOM element at `[data-range-id=index]`.
 */
type RangeInfoWithIndex = RangeInfo & {index: number};

/** Like DOMRect, but properties are mutable. */
type Rect = {
  top: number;
  left: number;
  width: number;
  height: number;
};

/**
 * Find the `RangeInfo` containing the given `pos` using binary search.
 * The `end` of the last `RangeInfo` is treated as inclusive.
 */
function findRangeInfo(infos: readonly RangeInfo[], pos: number): RangeInfoWithIndex | null {
  let start = 0;
  let end = infos.length;
  while (start < end) {
    // eslint-disable-next-line no-bitwise
    const mid = (start + end) >> 1;
    const info = infos[mid];
    const isEnd = mid === infos.length - 1 ? 1 : 0;
    if (info.start <= pos && pos < info.end + isEnd) {
      return {...info, index: mid};
    } else if (info.start > pos) {
      end = mid;
    } else {
      start = mid + 1;
    }
  }
  return null;
}

/** Restart the "blink" animation. */
function restartBlinking(element: Element) {
  for (const animation of element.getAnimations()) {
    if ((animation as CSSAnimation).animationName === 'blink') {
      animation.cancel();
      animation.play();
      return;
    }
  }
}

/**
 * Get the client rect of the intersection of `rangeInfo` and `start..end`.
 * The container must have `[data-range-id=index]` elements. This function
 * locates the related element by looking for `data-range-id` == `rangeInfo.index`.
 */
function getRangeRect(
  container: Element,
  rangeInfo: RangeInfoWithIndex,
  start: number,
  end: number,
): Rect | undefined {
  const span = container.querySelector(`[data-range-id="${rangeInfo.index}"]`);
  const textNode = span?.firstChild;
  if (textNode?.nodeType !== Node.TEXT_NODE) {
    return undefined;
  }
  const range = document.createRange();
  const textLen = textNode.textContent?.length ?? 0;
  range.setStart(textNode, Math.min(textLen, Math.max(start - rangeInfo.start, 0)));
  range.setEnd(textNode, Math.min(textLen, end - rangeInfo.start));
  return DOMRectToRect(range.getBoundingClientRect());
}

/**
 * word/line: selection is extended to word/line boundary.
 * This is used when you double/triple click then optionally drag select.
 */
type SelectionMode = 'char' | 'word' | 'line';

function nextSelectionMode(mode: SelectionMode): SelectionMode {
  if (mode === 'char') {
    return 'word';
  } else if (mode === 'word') {
    return 'line';
  } else {
    return 'char';
  }
}

/**
 * Extends the current textarea selection to match the `mode`.
 * `pos` is the current cursor position. It is used when `mode`
 * is `char` but the current textarea selection is a range.
 */
function extendTextareaSelection(
  textarea: HTMLTextAreaElement | null,
  mode: SelectionMode,
  pos: number,
): [number, number] {
  if (textarea == null || mode === 'char') {
    return [pos, pos];
  }
  const text = textarea.value;
  const start = textarea.selectionStart;
  const end = textarea.selectionEnd;
  return extendSelection(text, start, end, mode);
}

/** Extends the selection based on `SelectionMode`. */
function extendSelection(
  text: string,
  startPos: number,
  endPos: number,
  mode: SelectionMode,
): [number, number] {
  let start = startPos;
  let end = endPos;
  const charAt = (i: number) => text.substring(i, i + 1);
  const isNewLine = (i: number) => charAt(i) === '\n';
  if (mode === 'word') {
    const isWord = (i: number): boolean => charAt(i).match(/\w/) !== null;
    const isStartWord = isWord(start);
    while (start > 0 && !isNewLine(start - 1) && isWord(start - 1) === isStartWord) {
      start--;
    }
    while (end < text.length && !isNewLine(end) && isWord(end) === isStartWord) {
      end++;
    }
  } else if (mode === 'line') {
    while (start > 0 && !isNewLine(start - 1)) {
      start--;
    }
    while (end < text.length && !isNewLine(end)) {
      end++;
    }
  }
  return [start, end];
}

function getSelectedText(textarea: HTMLTextAreaElement): string {
  const {selectionStart, selectionEnd, value} = textarea;
  const text = value.substring(selectionStart, selectionEnd);
  return text;
}

/** Convert DOMRect to Rect. The latter has mutable properties. */
function DOMRectToRect(domRect: DOMRect | undefined): Rect | undefined {
  if (domRect === undefined) {
    return undefined;
  }
  const {width, height, top, left} = domRect;
  return {width, height, top, left};
}

/** Convert selections to `Rect`s relative to `container` for rendering. */
function selectionRangeToRects(
  rangeInfos: readonly RangeInfo[],
  container: Element,
  start: number,
  end: number,
): readonly Rect[] {
  if (start === end) {
    return [];
  }

  const clientRects: Rect[] = [];
  const startRangeInfo = findRangeInfo(rangeInfos, start);
  const endRangeInfo = findRangeInfo(rangeInfos, end);
  for (let i = startRangeInfo?.index ?? rangeInfos.length; i <= (endRangeInfo?.index ?? -1); i++) {
    const rect = getRangeRect(container, {...rangeInfos[i], index: i}, start, end);
    if (rect == null) {
      continue;
    }
    // For empty rect like "\n", make it wide enough to be visible.
    rect.width = Math.max(rect.width, 2);
    if (clientRects.length === 0) {
      clientRects.push(rect);
    } else {
      // Maybe merge with rects[-1].
      const lastRect = clientRects[clientRects.length - 1];
      const lastBottom = lastRect.top + lastRect.height;
      if (lastRect.top == rect.top || lastBottom == rect.top + rect.height) {
        lastRect.width =
          Math.max(lastRect.left + lastRect.width, rect.left + rect.width) - lastRect.left;
      } else {
        // Remove small gaps caused by line-height CSS property.
        const gap = rect.top - lastBottom;
        if (Math.abs(gap) < rect.height / 2) {
          lastRect.height += gap;
        }
        clientRects.push(rect);
      }
    }
  }
  const containerRect = container.getBoundingClientRect();
  const rects = clientRects.map(rect => {
    return {
      width: rect.width,
      height: rect.height,
      top: rect.top - containerRect.top,
      left: rect.left - containerRect.left,
    };
  });
  return rects;
}

type SelectionHighlightProps = {rects: readonly Rect[]};

function SelectionHighlight(props: SelectionHighlightProps) {
  return (
    <div className="text-editable-selection-highlight">
      {props.rects.map(rect => (
        <div key={`${rect.top}`} className="text-editable-selection-highlight-line" style={rect} />
      ))}
    </div>
  );
}

type CaretProps = {height: number; offsetX: number; offsetY: number};

function Caret(props: CaretProps) {
  const caretRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (caretRef.current !== null) {
      // Restart blinking when moved.
      restartBlinking(caretRef.current);
    }
  }, [props.offsetX, props.offsetY, caretRef]);

  const style = {
    height: props.height,
    transform: `translate(${props.offsetX}px, ${props.offsetY}px)`,
  };

  return <div className="text-editable-caret" ref={caretRef} style={style} />;
}

/**
 * Plain text editing backed by a hidden textarea.
 *
 * ## Usage
 *
 * Properties:
 * - `value`: The plain text value to edit. This should be the full text,
 *   with "context lines" expanded, so the user can Ctrl+A Ctrl+C copy it.
 * - `rangeInfos`: (start, end) information for rendered elements. See below.
 * - `children` with `[data-range-id]` elements. See below.
 * - `onTextChange` handler to update `value`.
 *
 * `RangeInfo[]` and `[data-range-id]` elements. For example:
 *
 * ```tsx
 * const value = "foo\nbar\nbaz\n";
 * const rangeInfos: RangeInfo[] = [
 *   {start: 0, end: 4},  // "foo\n", index = 0
 *   {start: 4, end: 8},  // "bar\n", index = 1
 *   {start: 8, end: 12}, // "baz\n", index = 2
 * ];
 *
 * const children = [
 *   <div key={0} data-range-id={0}>{"foo\n"}</div>
 *   <div key={1} className="collapsed">[Context line hidden]</div>
 *   <div key={2} data-range-id={2}>{"baz\n"}</div>
 * ];
 * ```
 *
 * The `rangeInfos` should cover the entire range of `value` and is sorted.
 * The `[data-range-id]` elements can be missing for ranges, this skips
 * rendering the related ranges, although the user can still Ctrl+A select,
 * and copy or edit them.
 *
 * ## Internals
 *
 * Layout:
 *
 * ```jsx
 *    <div>
 *      <textarea />
 *      <Caret /><SelectionHighlight />
 *      <Container>{children}</Container>
 *    </div>
 * ```
 *
 * Data flow:
 * - Text
 *   - `props.value` -> `<textarea />`
 *   - Keyboard on `<textarea />` -> `props.onTextChange` -> new `props.value`.
 *     (ex. typing, copy/paste/cut/undo/redo, IME input)
 *   - This component does not convert the `<container />` back to `value`.
 * - Selection
 *   - Keyboard on `<textarea />` -> `textarea.onSelect` -> `setCaretProps`.
 *     (ex. movements with arrow keys, ctrl+arrow keys, home/end, selections
 *     with shift+movement keys)
 *   - Mouse on `<Container />` -> `textarea.setSelectionRange` -> ...
 *     (ex. click to position, double or triple click to select word or line,
 *     drag to select a range)
 */
export function TextEditable(props: {
  children?: React.ReactNode;
  value: string;
  rangeInfos: readonly RangeInfo[];
  onTextChange?: (value: string) => void;
  onSelectChange?: (start: number, end: number) => void | [number, number];
}) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Event handler states.
  const [isPointerDown, setIsPointerDown] = useState(false);
  const pointerDownPos = useRef<number>(0);
  const selectionMode = useRef<SelectionMode>('char');

  // Caret and selection highlight states.
  const [focused, setFocused] = useState(false);
  const [caretProps, setCaretProps] = useState<CaretProps>({
    height: 0,
    offsetX: 0,
    offsetY: 0,
  });
  const [highlightProps, setHighlightProps] = useState<SelectionHighlightProps>({rects: []});

  /** Logic to recalculate caretProps and highlightProps.  */
  const recalculatePositions = () => {
    const textarea = textareaRef.current;
    const container = containerRef.current;
    if (textarea == null || container == null) {
      return;
    }

    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;

    const nextCaretProps = {...caretProps, height: 0};
    const nextHighlightProps = {...highlightProps};
    const containerRect = container.getBoundingClientRect();

    if (start === end) {
      const rangeInfo = findRangeInfo(props.rangeInfos, start);
      if (rangeInfo != null) {
        const caretRect = getRangeRect(container, rangeInfo, start, start);
        if (caretRect != null) {
          nextCaretProps.height = caretRect.height;
          nextCaretProps.offsetX = Math.floor(caretRect.left - containerRect.left);
          nextCaretProps.offsetY = Math.round(caretRect.top - containerRect.top);
        }
      }
      nextHighlightProps.rects = [];
    } else {
      nextHighlightProps.rects = selectionRangeToRects(props.rangeInfos, container, start, end);
      nextCaretProps.height = 0;
    }

    if (!deepEqual(caretProps, nextCaretProps)) {
      setCaretProps(nextCaretProps);
    }
    if (!deepEqual(highlightProps, nextHighlightProps)) {
      setHighlightProps(nextHighlightProps);
    }
  };

  /** Update caretProps, highlightProps on re-render and resize. */
  useLayoutEffect(() => {
    const container = containerRef.current;
    recalculatePositions();
    if (container == null) {
      return;
    }
    const observer = new ResizeObserver(() => {
      recalculatePositions();
    });
    observer.observe(container);
    return () => {
      observer.disconnect();
    };
  });

  /**
   * If `startEnd` is set, call `props.onSelectChange` with `startEnd`.
   * Otherwise, call `props.onSelectChange` with the current textarea selection.
   * `props.onSelectChange` might return a new selection to apply.
   */
  const setSelectRange = (startEnd?: [number, number]): [number, number] => {
    const textarea = textareaRef.current;
    const origStart = textarea?.selectionStart ?? 0;
    const origEnd = textarea?.selectionEnd ?? 0;
    const start = startEnd?.[0] ?? origStart;
    const end = startEnd?.[1] ?? origEnd;
    const [nextStart, nextEnd] = props.onSelectChange?.(start, end) || [start, end];
    if (textarea != null && (origStart !== nextStart || origEnd !== nextEnd)) {
      textarea.setSelectionRange(nextStart, nextEnd);
      // textarea onSelect fires after PointerUp. We want live updates during PointerDown/Move.
      recalculatePositions();
    }
    if (!focused) {
      setFocused(true);
    }
    return [nextStart, nextEnd];
  };

  /** Convert the pointer position to the text position. */
  const pointerToTextPos = (e: React.PointerEvent<Element>): number | undefined => {
    if (e.buttons !== 1) {
      return undefined;
    }
    let rangeElement = null;
    let offset = 0;
    // Firefox supports the "standard" caretPositionFromPoint.
    // TypeScript incorrectly removed it: https://github.com/microsoft/TypeScript/issues/49931
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const caretPositionFromPoint = (document as any).caretPositionFromPoint?.bind(document);
    if (caretPositionFromPoint) {
      const caret = caretPositionFromPoint(e.clientX, e.clientY);
      if (caret != null) {
        rangeElement = caret.offsetNode.parentElement;
        offset = caret.offset;
      }
    } else {
      // Chrome/WebKit only supports the "deprecated" caretRangeFromPoint.
      const range = document.caretRangeFromPoint(e.clientX, e.clientY);
      if (range != null) {
        rangeElement = range.startContainer.parentElement;
        offset = range.startOffset;
      }
    }
    const rangeId = rangeElement?.getAttribute('data-range-id');
    if (rangeId == null) {
      return;
    }
    const rangeInfo = props.rangeInfos[parseInt(rangeId)];
    return rangeInfo.start + offset;
  };

  const handleCopy = (e: React.ClipboardEvent<HTMLElement>) => {
    e.preventDefault();
    const textarea = textareaRef.current;
    if (textarea != null) {
      const text = getSelectedText(textarea);
      e.clipboardData.setData('text/plain', text);
    }
  };

  const handlePaste = (e: React.ClipboardEvent<HTMLElement>) => {
    e.preventDefault();
    const textarea = textareaRef.current;
    if (textarea != null) {
      const text = e.clipboardData.getData('text/plain');
      textarea.setRangeText(text);
    }
  };

  const handleCut = (e: React.ClipboardEvent<HTMLElement>) => {
    handleCopy(e);
    const textarea = textareaRef.current;
    if (textarea != null) {
      textarea.setRangeText('');
    }
  };

  /** Set the "start" selection, or extend selection on double/triple clicks. */
  const handlePointerDown = (e: React.PointerEvent<HTMLElement>) => {
    const pos = pointerToTextPos(e);
    if (pos == null) {
      return;
    }
    setIsPointerDown(true);
    // Shift + click does range selection.
    if (e.shiftKey) {
      handlePointerMove(e);
      // Prevent textarea's default Shift + click handling.
      e.stopPropagation();
      e.preventDefault();
      return;
    }
    // Double or triple click extends the selection.
    const isDoubleTripleClick = pos === pointerDownPos.current;
    if (isDoubleTripleClick) {
      selectionMode.current = nextSelectionMode(selectionMode.current);
    } else {
      selectionMode.current = 'char';
    }
    const [start, end] = (isDoubleTripleClick &&
      extendTextareaSelection(textareaRef.current, selectionMode.current, pos)) || [pos, pos];
    pointerDownPos.current = pos;
    setSelectRange([start, end]);
  };

  /** Set the "end" selection. */
  const handlePointerMove = (e: React.PointerEvent<HTMLElement>) => {
    const pos = pointerToTextPos(e);
    if (pos == null) {
      return;
    }
    const oldPos = pointerDownPos.current;
    const [start, end] = pos > oldPos ? [oldPos, pos] : [pos, oldPos];
    // Extend [start, end] by word/line selection.
    const textarea = textareaRef.current;
    const [newStart, newEnd] = extendSelection(
      textarea?.value ?? '',
      start,
      end,
      selectionMode.current,
    );
    setSelectRange([newStart, newEnd]);
  };

  /** Focus the hidden textarea so it handles keyboard events. */
  const handlePointerUpCancel = (_e: React.PointerEvent<HTMLElement>) => {
    // If pointerToTextPos returned null in the first place, do not set focus.
    if (isPointerDown) {
      textareaRef?.current?.focus();
    }
    setIsPointerDown(false);
  };

  /** Delegate text change to the callsite. */
  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    props.onTextChange?.(e.target.value);
  };

  /** Delegate selection change to the callsite. */
  const handleSelect = (_e: React.SyntheticEvent<HTMLTextAreaElement>) => {
    setSelectRange();
    recalculatePositions();
  };

  // When typing in a textarea, the browser might perform `scrollIntoView`.
  // Position the textarea to the bottom-right caret position so scrolling
  // works as expected. To test, pick a long change, then press arrow down
  // to move the cursor to the end.
  const textareaStyle = {
    transform: `translate(${caretProps.offsetX}px, ${caretProps.offsetY + caretProps.height}px)`,
  };

  return (
    // The "group" is used for positioning (relative -> absolute).
    // The PointerUp events are on the root element, not "container" to avoid issues
    // when "container" gets unmounted occasionally, losing the important PointerUp
    // events to set focus on textarea.
    <div
      className="text-editable-group"
      onPointerUp={handlePointerUpCancel}
      onPointerCancel={handlePointerUpCancel}>
      <div className="text-editable-overlay">
        {(focused || isPointerDown) && (
          <>
            <Caret {...caretProps} />
            <SelectionHighlight {...highlightProps} />
          </>
        )}
      </div>
      <textarea
        className="text-editable-hidden-textarea"
        ref={textareaRef}
        value={props.value}
        style={textareaStyle}
        onChange={handleChange}
        onSelect={handleSelect}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
      />
      <div
        className="text-editable-container"
        ref={containerRef}
        role="textbox"
        onDragStart={e => e.preventDefault()}
        onCopy={handleCopy}
        onPaste={handlePaste}
        onCut={handleCut}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}>
        {props.children}
      </div>
    </div>
  );
}
