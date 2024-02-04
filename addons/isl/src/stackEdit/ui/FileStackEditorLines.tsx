/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TokenizedHunk} from '../../ComparisonView/SplitDiffView/syntaxHighlighting';
import type {FlattenLine} from '../../linelog';
import type {FileStackState, Rev} from '../fileStackState';
import type {RangeInfo} from './TextEditable';

import {t} from '../../i18n';
import {bumpStackEditMetric} from './stackEditState';
import {Set as ImSet, Range} from 'immutable';
import {applyTokenizationToLine} from 'shared/createTokenizedIntralineDiff';
import {type Block, collapseContextBlocks, type LineIdx} from 'shared/diff';

export type ComputedFileStackLines = {
  leftGutter: JSX.Element[];
  leftButtons: JSX.Element[];
  mainContent: JSX.Element[];
  rightGutter: JSX.Element[];
  rightButtons: JSX.Element[];
  lineKind: Array<string>;
};

export type Mode = 'unified-diff' | 'side-by-side-diff' | 'unified-stack';

/**
 * Given
 * Compute content lines
 */
export function computeLinesForFileStackEditor(
  stack: FileStackState,
  setStack: (stack: FileStackState) => unknown,
  rev: Rev,
  mode: Mode,
  aLines: Array<string>,
  bLines: Array<string>,
  highlightedALines: TokenizedHunk | undefined,
  highlightedBLines: TokenizedHunk | undefined,
  abBlocks: Array<Block>,
  cbBlocks: Array<Block>,
  blocks: Array<Block>,
  expandedLines: ImSet<number>,
  setExpandedLines: (v: ImSet<number>) => unknown,
  selectedLineIds: ImSet<string>,
  rangeInfos: Array<RangeInfo>,
  readOnly: boolean,
  textEdit: boolean,
  aTextIsOverriden?: boolean,
): ComputedFileStackLines {
  const leftGutter: JSX.Element[] = [];
  const leftButtons: JSX.Element[] = [];
  const mainContent: JSX.Element[] = [];
  const rightGutter: JSX.Element[] = [];
  const rightButtons: JSX.Element[] = [];
  const lineKind: Array<string> = [];

  // Can move left? If `leftIsOverriden` is set (because copyFrom),
  // disabling moving left by setting leftMost to true.
  const leftMost = rev <= 1 || aTextIsOverriden === true;
  const rightMost = rev + 1 >= stack.revLength;

  // Utility to get the "different" block containing the given b-side line number.
  // Used by side-by-side diff to highlight left and right gutters.
  const buildGetDifferentBlockFunction = (blocks: Array<Block>) => {
    let blockIdx = 0;
    return (bIdx: LineIdx): Block | null => {
      while (blockIdx < blocks.length && bIdx >= blocks[blockIdx][1][3]) {
        blockIdx++;
      }
      return blockIdx < blocks.length && blocks[blockIdx][0] === '!' ? blocks[blockIdx] : null;
    };
  };
  const getLeftDifferentBlock = buildGetDifferentBlockFunction(abBlocks);
  const getRightDifferentBlock = buildGetDifferentBlockFunction(cbBlocks);
  const blockToClass = (block: Block | null, add = true): ' add' | ' del' | ' change' | '' =>
    block == null ? '' : block[1][0] === block[1][1] ? (add ? ' add' : ' del') : ' change';

  // Collapse unchanged context blocks, preserving the context lines.
  const collapsedBlocks = collapseContextBlocks(blocks, (_aLine, bLine) =>
    expandedLines.has(bLine),
  );

  const handleContextExpand = (b1: LineIdx, b2: LineIdx) => {
    const newSet = expandedLines.union(Range(b1, b2));
    setExpandedLines(newSet);
  };

  const showLineButtons = !textEdit && !readOnly && mode === 'unified-diff';
  const pushLineButtons = (sign: '=' | '!' | '~', aIdx?: LineIdx, bIdx?: LineIdx) => {
    if (!showLineButtons) {
      return;
    }

    let leftButton: JSX.Element | string = ' ';
    let rightButton: JSX.Element | string = ' ';

    // Move one or more lines. If the current line is part of the selection,
    // Move all lines in the selection.
    const moveLines = (revOffset: number) => {
      // Figure out which lines to move on both sides.
      let aIdxToMove: ImSet<LineIdx> = ImSet();
      let bIdxToMove: ImSet<LineIdx> = ImSet();
      if (
        (aIdx != null && selectedLineIds.has(`a${aIdx}`)) ||
        (bIdx != null && selectedLineIds.has(`b${bIdx}`))
      ) {
        // Move selected multiple lines.
        aIdxToMove = aIdxToMove.withMutations(mut => {
          let set = mut;
          selectedLineIds.forEach(id => {
            if (id.startsWith('a')) {
              set = set.add(parseInt(id.slice(1)));
            }
          });
          return set;
        });
        bIdxToMove = bIdxToMove.withMutations(mut => {
          let set = mut;
          selectedLineIds.forEach(id => {
            if (id.startsWith('b')) {
              set = set.add(parseInt(id.slice(1)));
            }
          });
          return set;
        });
      } else {
        // Move a single line.
        if (aIdx != null) {
          aIdxToMove = aIdxToMove.add(aIdx);
        }
        if (bIdx != null) {
          bIdxToMove = bIdxToMove.add(bIdx);
        }
      }

      // Actually move the lines.
      const aRev = rev - 1;
      const bRev = rev;
      let currentAIdx = 0;
      let currentBIdx = 0;
      let mutStack = stack;

      // When `aTextIsOverriden` is set (usually because "copyFrom"), the `aLines`
      // does not match `stack.getRev(aRev)`. The lines in `aIdxToMove` might not
      // exist in `stack`. To move `aIdxToMove` properly, patch `stack` to use the
      // `aLines` content temporarily.
      //
      // Practically, this is needed for moving deleted lines right for renamed
      // files. "moving left" is disabled for renamed files so it is ignored now.
      const needPatchARev = aTextIsOverriden && revOffset > 0 && aIdxToMove.size > 0;
      if (needPatchARev) {
        mutStack = mutStack.editText(aRev, aLines.join(''), false);
      }

      mutStack = mutStack.mapAllLines(line => {
        let newRevs = line.revs;
        let extraLine: undefined | FlattenLine = undefined;
        if (line.revs.has(aRev)) {
          // This is a deletion.
          if (aIdxToMove.has(currentAIdx)) {
            if (revOffset > 0) {
              // Move deletion right - add it in bRev.
              newRevs = newRevs.add(bRev);
            } else {
              // Move deletion left - drop it from aRev.
              newRevs = newRevs.remove(aRev);
            }
          }
          currentAIdx += 1;
        }
        if (line.revs.has(bRev)) {
          // This is an insertion.
          if (bIdxToMove.has(currentBIdx)) {
            if (revOffset > 0) {
              // Move insertion right - drop it in bRev.
              newRevs = newRevs.remove(bRev);
            } else {
              // Move insertion left - add it to aRev.
              // If it already exists in aRev (due to diff shifting), duplicate the line.
              if (newRevs.has(aRev)) {
                extraLine = line.set('revs', ImSet([aRev]));
              } else {
                newRevs = newRevs.add(aRev);
              }
            }
          }
          currentBIdx += 1;
        }
        const newLine = newRevs === line.revs ? line : line.set('revs', newRevs);
        return extraLine != null ? [extraLine, newLine] : [newLine];
      });

      if (needPatchARev) {
        mutStack = mutStack.editText(aRev, stack.getRev(aRev), false);
      }

      setStack(mutStack);

      bumpStackEditMetric('splitMoveLine');

      // deselect
      window.getSelection()?.removeAllRanges();
    };

    const selected =
      aIdx != null
        ? selectedLineIds.has(`a${aIdx}`)
        : bIdx != null
        ? selectedLineIds.has(`b${bIdx}`)
        : false;

    if (!leftMost && sign === '!') {
      const title = selected
        ? t('Move selected line changes left')
        : t('Move this line change left');
      leftButton = (
        <span className="button" role="button" title={title} onClick={() => moveLines(-1)}>
          ⬅
        </span>
      );
    }
    if (!rightMost && sign === '!') {
      const title = selected
        ? t('Move selected line changes right')
        : t('Move this line change right');
      rightButton = (
        <span className="button" role="button" title={title} onClick={() => moveLines(+1)}>
          ⮕
        </span>
      );
    }

    const className = selected ? 'selected' : '';

    leftButtons.push(
      <div key={leftButtons.length} className={`${className} left`}>
        {leftButton}
      </div>,
    );
    rightButtons.push(
      <div key={rightButtons.length} className={`${className} right`}>
        {rightButton}
      </div>,
    );
  };

  let start = 0;
  const nextRangeId = (len: number): number => {
    const id = rangeInfos.length;
    const end = start + len;
    rangeInfos.push({start, end});
    start = end;
    return id;
  };
  const bLineSpan = (bLine: string): JSX.Element => {
    if (!textEdit) {
      return <span>{bLine}</span>;
    }
    const id = nextRangeId(bLine.length);
    return <span data-range-id={id}>{bLine}</span>;
  };

  collapsedBlocks.forEach(([sign, [a1, a2, b1, b2]]) => {
    if (sign === '~') {
      // Context line.
      leftGutter.push(<div key={a1} className="lineno" />);
      rightGutter.push(<div key={b1} className="lineno" />);
      mainContent.push(
        <div key={b1} className="context-button" onClick={() => handleContextExpand(b1, b2)}>
          {' '}
        </div>,
      );
      lineKind.push('context');
      pushLineButtons(sign, a1, b1);
      if (textEdit) {
        // Still need to update rangeInfos.
        let len = 0;
        for (let bi = b1; bi < b2; ++bi) {
          len += bLines[bi].length;
        }
        nextRangeId(len);
      }
    } else if (sign === '=') {
      // Unchanged.
      for (let ai = a1; ai < a2; ++ai) {
        const bi = ai + b1 - a1;
        const leftIdx = mode === 'unified-diff' ? ai : bi;
        leftGutter.push(
          <div className="lineno" key={ai} data-span-id={`${rev}-${leftIdx}l`}>
            {leftIdx + 1}
          </div>,
        );
        rightGutter.push(
          <div className="lineno" key={bi} data-span-id={`${rev}-${bi}r`}>
            {bi + 1}
          </div>,
        );
        mainContent.push(
          <div key={bi} className="unchanged line">
            {highlightedBLines == null
              ? bLineSpan(bLines[bi])
              : applyTokenizationToLine(bLines[bi], highlightedBLines[bi])}
          </div>,
        );
        lineKind.push('context');
        pushLineButtons(sign, ai, bi);
      }
    } else if (sign === '!') {
      // Changed.
      if (mode === 'unified-diff') {
        // Deleted lines only show up in unified diff.
        for (let ai = a1; ai < a2; ++ai) {
          leftGutter.push(
            <div className="lineno" key={ai}>
              {ai + 1}
            </div>,
          );
          rightGutter.push(<div className="lineno" key={`a${ai}`} />);
          const selId = `a${ai}`;
          let className = 'del line';
          if (selectedLineIds.has(selId)) {
            className += ' selected';
          }

          pushLineButtons(sign, ai, undefined);
          mainContent.push(
            <div key={-ai} className={className} data-sel-id={selId}>
              {highlightedALines == null
                ? aLines[ai]
                : applyTokenizationToLine(aLines[ai], highlightedALines[ai])}
            </div>,
          );
          lineKind.push(className);
        }
      }
      for (let bi = b1; bi < b2; ++bi) {
        // Inserted lines show up in unified and side-by-side diffs.
        let leftClassName = 'lineno';
        if (mode === 'side-by-side-diff') {
          leftClassName += blockToClass(getLeftDifferentBlock(bi), true);
        }
        leftGutter.push(
          <div className={leftClassName} key={`b${bi}`} data-span-id={`${rev}-${bi}l`}>
            {mode === 'unified-diff' ? null : bi + 1}
          </div>,
        );
        let rightClassName = 'lineno';
        if (mode === 'side-by-side-diff') {
          rightClassName += blockToClass(getRightDifferentBlock(bi), false);
        }
        rightGutter.push(
          <div className={rightClassName} key={bi} data-span-id={`${rev}-${bi}r`}>
            {bi + 1}
          </div>,
        );
        const selId = `b${bi}`;
        let lineClassName = 'line';
        if (mode === 'unified-diff') {
          lineClassName += ' add';
        } else if (mode === 'side-by-side-diff') {
          const lineNoClassNames = leftClassName + rightClassName;
          for (const name of [' change', ' add', ' del']) {
            if (lineNoClassNames.includes(name)) {
              lineClassName += name;
              break;
            }
          }
        }
        if (selectedLineIds.has(selId)) {
          lineClassName += ' selected';
        }
        pushLineButtons(sign, undefined, bi);
        mainContent.push(
          <div key={bi} className={lineClassName} data-sel-id={selId}>
            {highlightedBLines == null
              ? bLineSpan(bLines[bi])
              : applyTokenizationToLine(bLines[bi], highlightedBLines[bi])}
          </div>,
        );
        lineKind.push(lineClassName);
      }
    }
  });

  return {
    leftGutter,
    leftButtons,
    mainContent,
    rightGutter,
    rightButtons,
    lineKind,
  };
}
