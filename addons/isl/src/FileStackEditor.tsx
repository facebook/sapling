/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FileStackState, Rev} from './stackEdit/fileStackState';
import type {LineIdx} from 'shared/diff';

import {Row, ScrollX, ScrollY} from './ComponentUtils';
import {Tooltip} from './Tooltip';
import {t} from './i18n';
import {Set as ImSet, Range} from 'immutable';
import {useState, useRef, useEffect} from 'react';
import {collapseContextBlocks, diffBlocks, splitLines} from 'shared/diff';
import {unwrap} from 'shared/utils';

import './FileStackEditor.css';

type Mode = 'unified-diff';

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
  getTitle: (rev: Rev) => string;

  /**
   * Skip editing (or showing) given revs.
   * This is usually to skip rev 0 (public, empty) if it is absent.
   * In the side-by-side mode, rev 0 is shown it it is an existing empty file
   * (introduced by a previous public commit). rev 0 is not shown if it is
   * absent, aka. rev 1 added the file.
   */
  skip: (rev: Rev) => boolean;

  /** Diff mode. */
  mode: Mode;
};

type EditorProps = EditorRowProps & {
  /** The rev in the stack to edit. */
  rev: Rev;
};

export function FileStackEditor(props: EditorProps) {
  const mainContentRef = useRef<HTMLPreElement | null>(null);
  const [expandedLines, setExpandedLines] = useState<ImSet<LineIdx>>(ImSet);
  const [selectedLineIds, setSelectedLineIds] = useState<ImSet<string>>(ImSet);
  const {stack, rev, setStack} = props;

  // Selection change is a document event, not a <pre> event.
  useEffect(() => {
    const handleSelect = () => {
      const selection = window.getSelection();
      if (
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
  }, []);

  // Diff with the left side.
  const bText = stack.getRev(rev);
  const bLines = splitLines(bText);
  const aLines = splitLines(stack.getRev(Math.max(0, rev - 1)));
  const blocks = diffBlocks(aLines, bLines);

  const leftMost = rev <= 1;
  const rightMost = rev + 1 >= stack.revLength;

  const collapsedBlocks = collapseContextBlocks(blocks, (_aLine, bLine) =>
    expandedLines.has(bLine),
  );

  // We render 3 columns as 3 <pre>s so they align vertically:
  // [left gutter] [main content] [right gutter].
  // The arrays below are the children of the <pre>s. One element per line per column.
  const leftGutter: JSX.Element[] = [];
  const mainContent: JSX.Element[] = [];
  const rightGutter: JSX.Element[] = [];

  const handleContextExpand = (b1: LineIdx, b2: LineIdx) => {
    const newSet = expandedLines.union(Range(b1, b2));
    setExpandedLines(newSet);
  };

  const lineButtons = (sign: '=' | '!' | '~', aIdx?: LineIdx, bIdx?: LineIdx): JSX.Element => {
    const leftButtons = [];
    const rightButtons = [];

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
      const newStack = stack.mapAllLines(line => {
        let newRevs = line.revs;
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
              newRevs = newRevs.add(aRev);
            }
          }
          currentBIdx += 1;
        }
        return newRevs === line.revs ? line : line.set('revs', newRevs);
      });
      setStack(newStack);
    };

    if (!leftMost && sign === '!') {
      leftButtons.push(
        <span
          className="button"
          role="button"
          key="<"
          title={t('Move left')}
          onClick={() => moveLines(-1)}>
          🡄
        </span>,
      );
    }
    if (!rightMost && sign === '!') {
      rightButtons.push(
        <span
          className="button"
          role="button"
          key=">"
          title={t('Move right')}
          onClick={() => moveLines(+1)}>
          🡆
        </span>,
      );
    }
    return (
      <>
        <span className="line-buttons right">{rightButtons} </span>
        <span className="line-buttons left">{leftButtons} </span>
      </>
    );
  };

  collapsedBlocks.forEach(([sign, [a1, a2, b1, b2]]) => {
    if (sign === '~') {
      // Context line.
      leftGutter.push(
        <div key={a1} className="lineno">
          {' '}
        </div>,
      );
      rightGutter.push(
        <div key={b1} className="lineno">
          {' '}
        </div>,
      );
      mainContent.push(
        <div key={b1} className="context-button" onClick={() => handleContextExpand(b1, b2)}>
          {' '}
        </div>,
      );
    } else if (sign === '=') {
      // Unchanged.
      for (let ai = a1; ai < a2; ++ai) {
        const bi = ai + b1 - a1;
        leftGutter.push(
          <div className="lineno" key={ai}>
            {ai + 1}
          </div>,
        );
        rightGutter.push(
          <div className="lineno" key={bi}>
            {bi + 1}
          </div>,
        );
        mainContent.push(
          <div key={bi} className="unchanged line">
            {lineButtons(sign, ai, bi)}
            {bLines[bi]}
          </div>,
        );
      }
    } else if (sign === '!') {
      // Changed.
      for (let ai = a1; ai < a2; ++ai) {
        leftGutter.push(
          <div className="lineno" key={ai}>
            {ai + 1}
          </div>,
        );
        rightGutter.push(
          <div className="lineno" key={`a${ai}`}>
            {' '}
          </div>,
        );
        const selId = `a${ai}`;
        let className = 'del line';
        if (selectedLineIds.has(selId)) {
          className += ' selected';
        }
        mainContent.push(
          <div key={-ai} className={className} data-sel-id={selId}>
            {lineButtons(sign, ai, undefined)}
            {aLines[ai]}
          </div>,
        );
      }
      for (let bi = b1; bi < b2; ++bi) {
        leftGutter.push(
          <div className="lineno" key={`b${bi}`}>
            {' '}
          </div>,
        );
        rightGutter.push(
          <div className="lineno" key={bi}>
            {bi + 1}
          </div>,
        );
        const selId = `b${bi}`;
        let className = 'add line';
        if (selectedLineIds.has(selId)) {
          className += ' selected';
        }
        mainContent.push(
          <div key={bi} className={className} data-sel-id={selId}>
            {lineButtons(sign, undefined, bi)}
            {bLines[bi]}
          </div>,
        );
      }
    }
  });

  return (
    <ScrollY hideBar={true} maxSize="70vh">
      <Row className="file-stack-editor">
        <pre className="column-left-gutter">{leftGutter}</pre>
        <pre className="main-content" ref={mainContentRef}>
          {mainContent}
        </pre>
        <pre className="column-right-gutter">{rightGutter}</pre>
      </Row>
    </ScrollY>
  );
}

export function FileStackEditorRow(props: EditorRowProps) {
  // skip rev 0, the "public" revision for unified diff.
  const revs = props.stack
    .revs()
    .slice(props.mode === 'unified-diff' ? 1 : 0)
    .filter(r => !props.skip(r));
  return (
    <ScrollX>
      <Row className="file-stack-editor-row">
        {revs.map(rev => {
          const title = props.getTitle(rev);
          const shortTitle = title.split('\n')[0];
          return (
            <div key={rev}>
              <Tooltip title={title}>
                <div className="filerev-title">{shortTitle}</div>
              </Tooltip>
              <FileStackEditor rev={rev} {...props} />
            </div>
          );
        })}
      </Row>
    </ScrollX>
  );
}
