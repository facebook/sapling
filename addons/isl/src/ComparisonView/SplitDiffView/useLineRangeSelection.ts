/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useCallback, useRef, useState, useEffect} from 'react';

export type LineRangeSelection = {
  startLine: number;
  endLine: number;
  side: 'LEFT' | 'RIGHT';
  path: string;
} | null;

/**
 * Hook for click-and-drag line range selection on diff line numbers.
 *
 * Interaction:
 * 1. mousedown on a .lineNumber-commentable td starts selection
 * 2. mousemove over other line numbers highlights the range
 * 3. mouseup ends selection and calls onRangeSelected
 * 4. Single click (no drag) = single-line (start === end)
 *
 * Returns event handlers to attach to the table element and
 * the current in-progress selection for visual highlighting.
 */
export function useLineRangeSelection(
  onRangeSelected: (
    startLine: number,
    endLine: number,
    side: 'LEFT' | 'RIGHT',
    path: string,
  ) => void,
) {
  const [activeSelection, setActiveSelection] = useState<LineRangeSelection>(null);
  const isDragging = useRef(false);
  const startInfo = useRef<{line: number; side: string; path: string} | null>(null);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    const td = (e.target as HTMLElement).closest('td.lineNumber-commentable');
    if (!td) {
      return;
    }

    const lineNumber = Number(td.getAttribute('data-line-number'));
    const side = td.getAttribute('data-side') as 'LEFT' | 'RIGHT';
    const path = td.getAttribute('data-path') ?? '';

    if (!lineNumber || !side) {
      return;
    }

    // Prevent text selection during drag
    e.preventDefault();

    isDragging.current = true;
    startInfo.current = {line: lineNumber, side, path};
    setActiveSelection({startLine: lineNumber, endLine: lineNumber, side, path});
  }, []);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (!isDragging.current || !startInfo.current) {
      return;
    }

    const td = (e.target as HTMLElement).closest('td.lineNumber-commentable');
    if (!td) {
      return;
    }

    const lineNumber = Number(td.getAttribute('data-line-number'));
    const side = td.getAttribute('data-side') as 'LEFT' | 'RIGHT';

    // Must be same side
    if (!lineNumber || side !== startInfo.current.side) {
      return;
    }

    const start = Math.min(startInfo.current.line, lineNumber);
    const end = Math.max(startInfo.current.line, lineNumber);

    setActiveSelection({
      startLine: start,
      endLine: end,
      side: startInfo.current.side as 'LEFT' | 'RIGHT',
      path: startInfo.current.path,
    });
  }, []);

  const handleMouseUp = useCallback(() => {
    if (!isDragging.current || !startInfo.current || !activeSelection) {
      isDragging.current = false;
      startInfo.current = null;
      return;
    }

    isDragging.current = false;
    const sel = activeSelection;

    // Call the callback with the final range
    onRangeSelected(sel.startLine, sel.endLine, sel.side, sel.path);

    // Clear selection state
    setActiveSelection(null);
    startInfo.current = null;
  }, [activeSelection, onRangeSelected]);

  // Clean up if mouse leaves the window during drag
  useEffect(() => {
    const handleGlobalMouseUp = () => {
      if (isDragging.current) {
        isDragging.current = false;
        setActiveSelection(null);
        startInfo.current = null;
      }
    };
    window.addEventListener('mouseup', handleGlobalMouseUp);
    return () => window.removeEventListener('mouseup', handleGlobalMouseUp);
  }, []);

  return {
    activeSelection,
    tableHandlers: {
      onMouseDown: handleMouseDown,
      onMouseMove: handleMouseMove,
      onMouseUp: handleMouseUp,
    },
  };
}

/**
 * Check if a line number is within the active selection range.
 * Used by SplitDiffRow to apply highlighting CSS.
 */
export function isLineInSelection(
  lineNumber: number | null,
  side: 'LEFT' | 'RIGHT',
  selection: {startLine: number; endLine: number; side: 'LEFT' | 'RIGHT'} | null,
): boolean {
  if (!selection || lineNumber == null) {
    return false;
  }
  return (
    side === selection.side &&
    lineNumber >= selection.startLine &&
    lineNumber <= selection.endLine
  );
}
