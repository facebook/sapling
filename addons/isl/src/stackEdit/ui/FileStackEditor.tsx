/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FileStackState, Rev} from '../fileStackState';
import type {Mode} from './FileStackEditorLines';
import type {RangeInfo} from './TextEditable';
import type {Block, LineIdx} from 'shared/diff';

import {CommitTitle} from '../../CommitTitle';
import {Row, ScrollX, ScrollY} from '../../ComponentUtils';
import {VSCodeCheckbox} from '../../VSCodeCheckbox';
import {FlattenLine} from '../../linelog';
import {computeLinesForFileStackEditor} from './FileStackEditorLines';
import {TextEditable} from './TextEditable';
import deepEqual from 'fast-deep-equal';
import {Set as ImSet, Range, List} from 'immutable';
import React, {useState, useRef, useEffect, useLayoutEffect} from 'react';
import {mergeBlocks, collapseContextBlocks, diffBlocks, splitLines} from 'shared/diff';
import {unwrap} from 'shared/utils';

import './FileStackEditor.css';

type EditorRowProps = {
  /**
   * File stack to edit.
   *
   * Note: the editor for rev 1 might want to diff against rev 0 and rev 2,
   * and might have buttons to move lines to other revs. So it needs to
   * know the entire stack.
   */
  stack: FileStackState;

  /** Function to update the stack. */
  setStack: (stack: FileStackState) => void;

  /** Function to get the "title" of a rev. */
  getTitle?: (rev: Rev) => string;

  /**
   * Skip editing (or showing) given revs.
   * This is usually to skip rev 0 (public, empty) if it is absent.
   * In the side-by-side mode, rev 0 is shown it it is an existing empty file
   * (introduced by a previous public commit). rev 0 is not shown if it is
   * absent, aka. rev 1 added the file.
   */
  skip?: (rev: Rev) => boolean;

  /** Diff mode. */
  mode: Mode;

  /** Whehter to enable text editing. This will disable conflicting features. */
  textEdit: boolean;
};

type EditorProps = EditorRowProps & {
  /** The rev in the stack to edit. */
  rev: Rev;
};

export function FileStackEditor(props: EditorProps) {
  const mainContentRef = useRef<HTMLPreElement | null>(null);
  const [expandedLines, setExpandedLines] = useState<ImSet<LineIdx>>(ImSet);
  const [selectedLineIds, setSelectedLineIds] = useState<ImSet<string>>(ImSet);
  const [widthStyle, setWidthStyle] = useState<string>('unset');
  const {stack, rev, setStack, mode} = props;
  const readOnly = rev === 0;
  const textEdit = !readOnly && props.textEdit;
  const rangeInfos: RangeInfo[] = [];

  // Selection change is a document event, not a <pre> event.
  useEffect(() => {
    const handleSelect = () => {
      const selection = window.getSelection();
      if (
        textEdit ||
        selection == null ||
        mainContentRef.current == null ||
        !mainContentRef.current.contains(selection.anchorNode)
      ) {
        setSelectedLineIds(ids => (ids.isEmpty() ? ids : ImSet()));
        return;
      }
      const divs = mainContentRef.current.querySelectorAll<HTMLDivElement>('div[data-sel-id]');
      const selIds: Array<string> = [];
      for (const div of divs) {
        const child = div.lastChild;
        if (child && selection.containsNode(child, true)) {
          selIds.push(unwrap(div.dataset.selId));
        }
      }
      setSelectedLineIds(ImSet(selIds));
    };
    document.addEventListener('selectionchange', handleSelect);
    return () => {
      document.removeEventListener('selectionchange', handleSelect);
    };
  }, [textEdit]);

  if (mode === 'unified-stack') {
    return null;
  }

  // Diff with the left side.
  const bText = stack.getRev(rev);
  const bLines = splitLines(bText);
  const aLines = splitLines(stack.getRev(Math.max(0, rev - 1)));
  const abBlocks = diffBlocks(aLines, bLines);

  const rightMost = rev + 1 >= stack.revLength;

  // For side-by-side diff, we also need to diff with the right side.
  let cbBlocks: Array<Block> = [];
  let blocks = abBlocks;
  if (!rightMost && mode === 'side-by-side-diff') {
    const cText = stack.getRev(rev + 1);
    const cLines = splitLines(cText);
    cbBlocks = diffBlocks(cLines, bLines);
    blocks = mergeBlocks(abBlocks, cbBlocks);
  }

  const {leftGutter, leftButtons, mainContent, rightGutter, rightButtons} =
    computeLinesForFileStackEditor(
      stack,
      setStack,
      rev,
      mode,
      aLines,
      bLines,
      undefined,
      undefined,
      abBlocks,
      cbBlocks,
      blocks,
      expandedLines,
      setExpandedLines,
      selectedLineIds,
      rangeInfos,
      textEdit,
      readOnly,
    );

  const ribbons: Array<JSX.Element> = [];
  if (mode === 'side-by-side-diff' && rev > 0) {
    abBlocks.forEach(([sign, [a1, a2, b1, b2]]) => {
      if (sign === '!') {
        ribbons.push(
          <Ribbon
            a1={`${rev - 1}-${a1}r`}
            a2={`${rev - 1}-${a2 - 1}r`}
            b1={`${rev}-${b1}l`}
            b2={`${rev}-${b2 - 1}l`}
            outerContainerClass="file-stack-editor-outer-scroll-y"
            innerContainerClass="file-stack-editor"
            key={b1}
            className={b1 === b2 ? 'del' : a1 === a2 ? 'add' : 'change'}
          />,
        );
      }
    });
  }

  const handleTextChange = (value: string) => {
    const newStack = stack.editText(rev, value);
    setStack(newStack);
  };

  const handleXScroll: React.UIEventHandler<HTMLDivElement> = e => {
    // Dynamically decide between 'width: fit-content' and 'width: unset'.
    // Affects the position of the [->] "move right" button and the width
    // of the line background for LONG LINES.
    //
    //     |ScrollX width|
    // ------------------------------------------------------------------------
    //     |Editor width |              <- width: unset && scrollLeft == 0
    //     |Text width - could be long|    text could be longer
    //     |         [->]|                 "move right" button is visible
    // ------------------------------------------------------------------------
    // |Editor width |                  <- width: unset && scrollLeft > 0
    // |+/- highlight|                     +/- background covers partial text
    // |         [->]|                     "move right" at wrong position
    // ------------------------------------------------------------------------
    // |Editor width              | <- width: fit-content && scrollLeft > 0
    // |Text width - could be long|    long text width = editor width
    // |+/- highlight             |    +/- background covers all text
    // |                      [->]|    "move right" at the right side of text
    //
    const newWidthStyle = e.currentTarget?.scrollLeft > 0 ? 'fit-content' : 'unset';
    setWidthStyle(newWidthStyle);
  };

  const mainStyle: React.CSSProperties = {width: widthStyle};
  const mainContentPre = (
    <pre className="main-content" style={mainStyle} ref={mainContentRef}>
      {mainContent}
    </pre>
  );

  const showLineButtons = !textEdit && !readOnly && mode === 'unified-diff';

  return (
    <div className="file-stack-editor-ribbon-no-clip">
      {ribbons}
      <div>
        <Row className="file-stack-editor">
          {showLineButtons && <pre className="column-left-buttons">{leftButtons}</pre>}
          <pre className="column-left-gutter">{leftGutter}</pre>
          <ScrollX hideBar={true} size={500} maxSize={500} onScroll={handleXScroll}>
            {textEdit ? (
              <TextEditable value={bText} rangeInfos={rangeInfos} onTextChange={handleTextChange}>
                {mainContentPre}
              </TextEditable>
            ) : (
              mainContentPre
            )}
          </ScrollX>
          <pre className="column-right-gutter">{rightGutter}</pre>
          {showLineButtons && <pre className="column-right-buttons">{rightButtons}</pre>}
        </Row>
      </div>
    </div>
  );
}

/** The unified stack view is different from other views. */
function FileStackEditorUnifiedStack(props: EditorRowProps) {
  type ClickPosition = {
    rev: Rev;
    lineIdx: LineIdx;
    checked?: boolean;
  };
  const [clickStart, setClickStart] = useState<ClickPosition | null>(null);
  const [clickEnd, setClickEnd] = useState<ClickPosition | null>(null);
  const [expandedLines, setExpandedLines] = useState<ImSet<LineIdx>>(ImSet);

  const {stack, setStack, textEdit} = props;
  const {skip, getTitle} = getSkipGetTitleOrDefault(props);

  const rangeInfos: Array<RangeInfo> = [];

  const lines = stack.convertToFlattenLines();
  const revs = stack.revs().filter(rev => !skip(rev));
  const lastRev = revs.at(-1) ?? -1;

  // RangeInfo handling required by TextEditable.
  let start = 0;
  const nextRangeId = (len: number): number => {
    const id = rangeInfos.length;
    const end = start + len;
    rangeInfos.push({start, end});
    start = end;
    return id;
  };

  // Append `baseName` with `color${rev % 4}`.
  const getColorClassName = (baseName: string, rev: number): string => {
    const colorIdx = rev % 4;
    return `${baseName} color${colorIdx}`;
  };

  // Header. Commit titles.
  const headerRows = revs.map(rev => {
    const padTds = revs.map(rev2 => (
      <th key={rev2} className={getColorClassName('pad', Math.min(rev2, rev))}></th>
    ));
    const title = getTitle(rev);
    return (
      <tr key={rev}>
        {padTds}
        <th className={getColorClassName('commit-title', rev)}>
          <CommitTitle commitMessage={title} tooltipPlacement="left" />
        </th>
      </tr>
    );
  });

  // Checkbox range selection.
  const getSelRanges = (start: ClickPosition | null, end: ClickPosition | null) => {
    // Minimal number sort. Note Array.sort is a string sort.
    const sort2 = (a: number, b: number) => (a < b ? [a, b] : [b, a]);

    // Selected range to highlight.
    let lineRange = Range(0, 0);
    let revRange = Range(0, 0);
    if (start != null && end != null) {
      const [rev1, rev2] = sort2(start.rev, end.rev);
      // Skip rev 0 (public, immutable).
      revRange = Range(Math.max(rev1, 1), rev2 + 1);
      const [lineIdx1, lineIdx2] = sort2(start.lineIdx, end.lineIdx);
      lineRange = Range(lineIdx1, lineIdx2 + 1);
    }
    return [lineRange, revRange];
  };
  const [selLineRange, selRevRange] = getSelRanges(clickStart, clickEnd ?? clickStart);

  const handlePointerDown = (
    lineIdx: LineIdx,
    rev: Rev,
    checked: boolean,
    e: React.PointerEvent,
  ) => {
    if (e.isPrimary) {
      setClickStart({lineIdx, rev, checked});
    }
  };
  const handlePointerMove = (lineIdx: LineIdx, rev: Rev, e: React.PointerEvent) => {
    if (e.isPrimary && clickStart != null) {
      const newClickEnd = {lineIdx, rev, checked: false};
      setClickEnd(v => (deepEqual(v, newClickEnd) ? v : newClickEnd));
    }
  };
  const handlePointerUp = (lineIdx: LineIdx, rev: Rev, e: React.PointerEvent) => {
    setClickEnd(null);
    if (e.isPrimary && clickStart != null) {
      const [lineRange, revRange] = getSelRanges(clickStart, {lineIdx, rev});
      setClickStart(null);
      const newStack = stack.mapAllLines((line, i) => {
        if (lineRange.contains(i)) {
          const newRevs = clickStart.checked
            ? line.revs.union(revRange)
            : line.revs.subtract(revRange);
          return line.set('revs', newRevs);
        } else {
          return line;
        }
      });
      setStack(newStack);
    }
  };

  // Context line analysis. We "abuse" the `collapseContextBlocks` by faking the `blocks`.
  const blocks: Array<Block> = [];
  const pushSign = (sign: '!' | '=', end: LineIdx) => {
    const lastBlock = blocks.at(-1);
    if (lastBlock == null) {
      blocks.push([sign, [0, end, 0, end]]);
    } else if (lastBlock[0] === sign) {
      lastBlock[1][1] = lastBlock[1][3] = end;
    } else {
      blocks.push([sign, [lastBlock[1][1], end, lastBlock[1][3], end]]);
    }
  };
  lines.forEach((line, i) => {
    const sign = line.revs.size >= revs.length ? '=' : '!';
    pushSign(sign, i + 1);
  });
  const collapsedBlocks = collapseContextBlocks(blocks, (_a, b) => expandedLines.contains(b));

  const handleContextExpand = (b1: LineIdx, b2: LineIdx) => {
    const newSet = expandedLines.union(Range(b1, b2));
    setExpandedLines(newSet);
  };

  // Body. Checkboxes + Line content, or "~~~~" context button.
  const bodyRows: JSX.Element[] = [];
  collapsedBlocks.forEach(([sign, [, , b1, b2]]) => {
    if (sign === '~') {
      const checkboxes = revs.map(rev => (
        <td key={rev} className={getColorClassName('', rev)}></td>
      ));

      bodyRows.push(
        <tr key={b1}>
          {checkboxes}
          <td className="context-button" onClick={() => handleContextExpand(b1, b2)}>
            <span> </span>
          </td>
        </tr>,
      );

      if (textEdit) {
        const len = Range(b1, b2).reduce((acc, i) => acc + unwrap(lines.get(i)).data.length, 0);
        nextRangeId(len);
      }

      return;
    }
    for (let i = b1; i < b2; ++i) {
      const line = unwrap(lines.get(i));
      const checkboxes = revs.map(rev => {
        const checked = line.revs.contains(rev);
        let className = 'checkbox' + (rev > 0 ? ' mutable' : ' immutable');
        if (selLineRange.contains(i) && selRevRange.contains(rev)) {
          className += clickStart?.checked ? ' add' : ' del';
        }
        return (
          <td
            key={rev}
            className={getColorClassName(className, rev)}
            onPointerDown={e => handlePointerDown(i, rev, !checked, e)}
            onPointerMove={e => handlePointerMove(i, rev, e)}
            onPointerUp={e => handlePointerUp(i, rev, e)}
            onDragStart={e => e.preventDefault()}>
            <VSCodeCheckbox
              tabIndex={-1}
              disabled={rev === 0}
              checked={checked}
              style={{pointerEvents: 'none'}}
            />
          </td>
        );
      });
      let tdClass = 'line';
      if (!line.revs.has(lastRev)) {
        tdClass += ' del';
      } else if (line.revs.size < revs.length) {
        tdClass += ' change';
      }
      const rangeId = textEdit ? nextRangeId(line.data.length) : undefined;
      bodyRows.push(
        <tr key={i}>
          {checkboxes}
          <td className={tdClass}>
            <span className="line" data-range-id={rangeId}>
              {line.data}
            </span>
          </td>
        </tr>,
      );
    }
  });

  let editor = (
    <table className="file-unified-stack-editor">
      <thead>{headerRows}</thead>
      <tbody>{bodyRows}</tbody>
    </table>
  );

  if (textEdit) {
    const textLines = lines.map(l => l.data).toArray();
    const text = textLines.join('');
    const handleTextChange = (newText: string) => {
      const immutableRev = 0;
      const immutableRevs: ImSet<Rev> = ImSet([immutableRev]);
      const newTextLines = splitLines(newText);
      const blocks = diffBlocks(textLines, newTextLines);
      const newFlattenLines: List<FlattenLine> = List<FlattenLine>().withMutations(mut => {
        let flattenLines = mut;
        blocks.forEach(([sign, [a1, a2, b1, b2]]) => {
          if (sign === '=') {
            flattenLines = flattenLines.concat(lines.slice(a1, a2));
          } else if (sign === '!') {
            // Plain text does not have "revs" info.
            // We just reuse the last line on the a-side. This should work fine for
            // single-line insertion or edits.
            const fallbackRevs: ImSet<Rev> =
              lines.get(Math.max(a1, a2 - 1))?.revs?.delete(immutableRev) ?? ImSet();
            // Public (immutableRev, rev 0) lines cannot be deleted. Enforce that.
            const aLines = Range(a1, a2)
              .map(ai => lines.get(ai))
              .filter(l => l != null && l.revs.contains(immutableRev))
              .map(l => (l as FlattenLine).set('revs', immutableRevs));
            // Newly added lines cannot insert to (immutableRev, rev 0) either.
            const bLines = Range(b1, b2).map(bi => {
              const data = newTextLines[bi] ?? '';
              const ai = bi - b1 + a1;
              const revs =
                (ai < a2 ? lines.get(ai)?.revs?.delete(immutableRev) : null) ?? fallbackRevs;
              return FlattenLine({data, revs});
            });
            flattenLines = flattenLines.concat(aLines).concat(bLines);
          }
        });
        return flattenLines;
      });
      const newStack = stack.fromFlattenLines(newFlattenLines, stack.revLength);
      setStack(newStack);
    };

    editor = (
      <TextEditable rangeInfos={rangeInfos} value={text} onTextChange={handleTextChange}>
        {editor}
      </TextEditable>
    );
  }

  return <ScrollY maxSize="calc((100vh / var(--zoom)) - 300px)">{editor}</ScrollY>;
}

export function FileStackEditorRow(props: EditorRowProps) {
  if (props.mode === 'unified-stack') {
    return FileStackEditorUnifiedStack(props);
  }

  // skip rev 0, the "public" revision for unified diff.
  const {skip, getTitle} = getSkipGetTitleOrDefault(props);
  const revs = props.stack
    .revs()
    .slice(props.mode === 'unified-diff' ? 1 : 0)
    .filter(r => !skip(r));
  return (
    <ScrollX>
      <Row className="file-stack-editor-row">
        {revs.map(rev => {
          const title = getTitle(rev);
          return (
            <div key={rev}>
              <CommitTitle className="filerev-title" commitMessage={title} />
              <FileStackEditor rev={rev} {...props} />
            </div>
          );
        })}
      </Row>
    </ScrollX>
  );
}

function getSkipGetTitleOrDefault(props: EditorRowProps): {
  skip: (rev: Rev) => boolean;
  getTitle: (rev: Rev) => string;
} {
  const skip = props.skip ?? ((rev: Rev) => rev === 0);
  const getTitle = props.getTitle ?? (() => '');
  return {skip, getTitle};
}

/**
 * The "connector" between two editors.
 *
 * Takes 4 data-span-id attributes:
 *
 * +------------+        +------------+
 * | containerA |        | containerB |
 * |       +----+~~~~~~~~+----+       |
 * |       | a1 |        | b1 |       |
 * |       +----+        +----+       |
 * |       | .. | Ribbon | .. |       |
 * |       +----+        +----+       |
 * |       | a2 |        | b2 |       |
 * |       +----+~~~~~~~~+----+       |
 * |            |        |            |
 * +------------+        +------------+
 *
 * The ribbon is positioned relative to (outer) containerB,
 * the editor on the right side.
 *
 * The ribbon position will be recalculated if either containerA
 * or containerB gets resized or scrolled. Note there are inner
 * and outer containers. The scroll check is on the outer container
 * with the `overflow-y: auto`. The resize check is on the inner
 * container, since the outer container remains the same size
 * once overflowed.
 *
 * The ribbons are drawn outside the scroll container, and need
 * another container to have the `overflow: visible` behavior,
 * like:
 *
 *   <div style={{overflow: 'visible', position: 'relative'}}>
 *     <Ribbon />
 *     <ScrollY className="outerContainer">
 *        <Something className="innerContainer" />
 *     </ScrollY>
 *   </div>
 *
 * If one of a1 and a2 is missing, the a-side range is then
 * considered zero-height. This is useful for pure deletion
 * or insertion. Same for b1 and b2.
 */
function Ribbon(props: {
  a1: string;
  a2: string;
  b1: string;
  b2: string;
  outerContainerClass: string;
  innerContainerClass: string;
  className: string;
}) {
  type RibbonPos = {
    top: number;
    width: number;
    height: number;
    path: string;
  };
  const [pos, setPos] = useState<RibbonPos | null>(null);
  type E = HTMLElement;

  type Containers = {
    resize: E[];
    scroll: E[];
  };

  useLayoutEffect(() => {
    // Get the container elements and recaluclate positions.
    // Returns an empty array if the containers are not found.
    const repositionAndGetContainers = (): Containers | undefined => {
      // Find a1, a2, b1, b2. a2 and b2 are nullable.
      const select = (spanId: string): E | null =>
        spanId === ''
          ? null
          : document.querySelector(`.${props.outerContainerClass} [data-span-id="${spanId}"]`);
      const [a1, a2, b1, b2] = [props.a1, props.a2, props.b1, props.b2].map(select);
      const aEither = a1 ?? a2;
      const bEither = b1 ?? b2;
      if (aEither == null || bEither == null) {
        return;
      }

      // Find containers.
      const findContainer = (span: E, className: string): E | null => {
        for (let e: E | null = span; e != null; e = e.parentElement) {
          if (e.classList.contains(className)) {
            return e;
          }
        }
        return null;
      };
      const [outerA, outerB] = [aEither, bEither].map(e =>
        findContainer(e, props.outerContainerClass),
      );
      const [innerA, innerB] = [aEither, bEither].map(e =>
        findContainer(e, props.innerContainerClass),
      );
      if (outerA == null || outerB == null || innerA == null || innerB == null) {
        return;
      }

      // Recalculate positions. a2Rect and b2Rect are nullable.
      let newPos: RibbonPos | null = null;
      const [outerARect, outerBRect] = [outerA, outerB].map(e => e.getBoundingClientRect());
      const [a1Rect, a2Rect, b1Rect, b2Rect] = [a1, a2, b1, b2].map(
        e => e && e.getBoundingClientRect(),
      );
      const aTop = a1Rect?.top ?? a2Rect?.bottom;
      const bTop = b1Rect?.top ?? b2Rect?.bottom;
      const aBottom = a2Rect?.bottom ?? aTop;
      const bBottom = b2Rect?.bottom ?? bTop;
      const aRight = a1Rect?.right ?? a2Rect?.right;
      const bLeft = b1Rect?.left ?? b2Rect?.left;

      if (
        aTop != null &&
        bTop != null &&
        aBottom != null &&
        bBottom != null &&
        aRight != null &&
        bLeft != null
      ) {
        const top = Math.min(aTop, bTop) - outerBRect.top;
        const width = bLeft - aRight;
        const ay1 = Math.max(aTop - bTop, 0);
        const by1 = Math.max(bTop - aTop, 0);
        const height = Math.max(aBottom, bBottom) - Math.min(aTop, bTop);
        const ay2 = ay1 + aBottom - aTop;
        const by2 = by1 + bBottom - bTop;
        const mid = width / 2;

        // Discard overflow position.
        if (
          top >= 0 &&
          top + Math.max(ay2, by2) <= Math.max(outerARect.height, outerBRect.height)
        ) {
          const path = [
            `M 0 ${ay1}`,
            `C ${mid} ${ay1}, ${mid} ${by1}, ${width} ${by1}`,
            `L ${width} ${by2}`,
            `C ${mid} ${by2}, ${mid} ${ay2}, 0 ${ay2}`,
            `L 0 ${ay1}`,
          ].join(' ');
          newPos = {
            top,
            width,
            height,
            path,
          };
        }
      }

      setPos(pos => (deepEqual(pos, newPos) ? pos : newPos));

      return {
        scroll: [outerA, outerB],
        resize: [innerA, innerB],
      };
    };

    // Calcualte position now.
    const containers = repositionAndGetContainers();

    if (containers == null) {
      return;
    }

    // Observe resize and scrolling changes of the container.
    const observer = new ResizeObserver(() => repositionAndGetContainers());
    const handleScroll = () => {
      repositionAndGetContainers();
    };
    containers.resize.forEach(c => observer.observe(c));
    containers.scroll.forEach(c => c.addEventListener('scroll', handleScroll));

    return () => {
      observer.disconnect();
      containers.scroll.forEach(c => c.removeEventListener('scroll', handleScroll));
    };
  }, [
    props.a1,
    props.a2,
    props.b1,
    props.b2,
    props.outerContainerClass,
    props.innerContainerClass,
    props.className,
  ]);

  if (pos == null) {
    return null;
  }

  const style: React.CSSProperties = {
    top: pos.top,
    left: 1 - pos.width,
    width: pos.width,
    height: pos.height,
  };

  return (
    <svg className={`ribbon ${props.className}`} style={style}>
      <path d={pos.path} />
    </svg>
  );
}
