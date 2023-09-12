/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitStackState} from './stackEdit/commitStackState';
import type {FileStackState, Rev} from './stackEdit/fileStackState';
import type {RepoPath} from 'shared/types/common';

import {CommitTitle} from './CommitTitle';
import {FileHeader} from './ComparisonView/SplitDiffView/SplitDiffFileHeader';
import {Column, FlexRow, Row, ScrollX, ScrollY} from './ComponentUtils';
import {EmptyState} from './EmptyState';
import {Subtle} from './Subtle';
import {t, T} from './i18n';
import {SplitRangeRecord, useStackEditState} from './stackEditState';
import {
  VSCodeDivider,
  VSCodeDropdown,
  VSCodeOption,
  VSCodeTextField,
} from '@vscode/webview-ui-toolkit/react';
import {Set as ImSet, Range, Seq} from 'immutable';
import {useRef, useState, useEffect} from 'react';
import {Icon} from 'shared/Icon';
import {type LineIdx, splitLines, diffBlocks, collapseContextBlocks} from 'shared/diff';
import {DiffType} from 'shared/patch/parse';
import {unwrap} from 'shared/utils';

import './VSCodeDropdown.css';
import './SplitStackEditPanel.css';

export function SplitStackEditPanel() {
  const stackEdit = useStackEditState();

  const {commitStack} = stackEdit;

  // Find the commits being split.
  const [startRev, endRev] = findStartEndRevs(commitStack, stackEdit.splitRange);

  // Nothing to split? Show a dropdown.
  if (startRev == null || endRev == null || startRev > endRev) {
    return (
      <div>
        <StackRangeSelector />
        <EmptyState>
          <T>Select a commit to split its changes.</T>
          <br />
          <T>Select a range of commits to move contents among them.</T>
        </EmptyState>
      </div>
    );
  }

  // Prepare a "dense" subStack with an extra empty commit to move right.
  const emptyTitle = getEmptyCommitTitle(commitStack.get(endRev)?.text ?? '');
  const subStack = commitStack
    .insertEmpty(endRev + 1, emptyTitle)
    .denseSubStack(Range(startRev, endRev + 2).toList());

  // One commit per column.
  const columns: JSX.Element[] = subStack
    .revs()
    .map(rev => <SplitColumn key={rev} rev={rev} subStack={subStack} />);

  // Remove the padding/margin set by the panel.
  // Useful for long ScrollX content.
  const negativeMargin: React.CSSProperties = {
    marginLeft: 'calc(0px - var(--pad))',
    marginRight: 'calc(0px - var(--pad))',
  };

  return (
    <>
      <StackRangeSelector />
      <div
        style={{
          ...negativeMargin,
          marginBottom: 'var(--pad)',
          borderBottom: '1px dashed var(--tooltip-border)',
        }}
      />
      <div
        style={{
          ...negativeMargin,
          minWidth: 'calc(100vw - 81px)',
          minHeight: 'calc(100vh - 270px)',
        }}>
        <ScrollX maxSize="calc(100vw - 50px)">
          <Row style={{padding: '0 var(--pad)'}}>{columns}</Row>
        </ScrollX>
      </div>
    </>
  );
}

type SplitColumnProps = {
  subStack: CommitStackState;
  rev: Rev;
};

function SplitColumn(props: SplitColumnProps) {
  const {subStack, rev} = props;

  const commit = subStack.get(rev);
  const commitMessage = commit?.text ?? '';
  const sortedFileStacks = subStack.fileStacks
    .map((fileStack, fileIdx): [RepoPath, FileStackState, Rev] => {
      return [subStack.getFileStackPath(fileIdx, 0) ?? '', fileStack, fileIdx];
    })
    .sortBy(t => t[0]);

  const editors = sortedFileStacks.flatMap(([path, fileStack, fileIdx]) => {
    // subStack is a "dense" stack. fileRev is commitRev + 1.
    const fileRev = rev + 1;
    const isModified = fileRev > 0 && fileStack.getRev(fileRev - 1) !== fileStack.getRev(fileRev);
    const editor = (
      <SplitEditorWithTitle
        key={path}
        subStack={subStack}
        path={path}
        fileStack={fileStack}
        fileIdx={fileIdx}
        fileRev={fileRev}
      />
    );
    return Seq(isModified ? [editor] : []);
  });

  const body = editors.isEmpty() ? (
    <EmptyState small>
      <Column>
        <T>This commit is empty</T>
        <Subtle>
          <T>Use the left/right arrows to move files and lines of code and create new commits.</T>
        </Subtle>
      </Column>
    </EmptyState>
  ) : (
    <ScrollY maxSize="calc(100vh - 350px)" hideBar={true}>
      {editors}
    </ScrollY>
  );

  // The min width ensures it does not look too narrow for an empty commit.
  return (
    <div className="split-commit-column" style={{minWidth: 300, flexShrink: 0}}>
      <div className="split-commit-header">
        <span className="split-commit-header-stack-number">
          {rev + 1} / {subStack.size}
        </span>
        <MaybeEditableCommitTitle commitMessage={commitMessage} commitKey={commit?.key} />
      </div>
      {body}
    </div>
  );
}

type SplitEditorWithTitleProps = {
  subStack: CommitStackState;
  path: RepoPath;
  fileStack: FileStackState;
  fileIdx: number;
  fileRev: Rev;
};

function SplitEditorWithTitle(props: SplitEditorWithTitleProps) {
  const stackEdit = useStackEditState();

  const {commitStack} = stackEdit;
  const {subStack, path, fileStack, fileIdx, fileRev} = props;

  const setStack = (newFileStack: FileStackState) => {
    const newSubStack = subStack.setFileStack(fileIdx, newFileStack);
    const [startRev, endRev] = findStartEndRevs(commitStack, stackEdit.splitRange);
    if (startRev != null && endRev != null) {
      const newCommitStack = commitStack.applySubStack(startRev, endRev + 1, newSubStack);
      // Find the new split range.
      const endOffset = newCommitStack.size - commitStack.size;
      const startKey = newCommitStack.get(startRev)?.key ?? '';
      const endKey = newCommitStack.get(endRev + endOffset)?.key ?? '';
      const splitRange = SplitRangeRecord({startKey, endKey});
      // Update the main stack state.
      stackEdit.push(newCommitStack, {name: 'split', path}, splitRange);
    }
  };

  return (
    <div className="split-commit-file">
      <FileHeader
        path={path}
        diffType={DiffType.Modified}
        fileActions={<>{/* TODO: move entire file left/right */}</>}
      />
      <SplitFile key={fileIdx} rev={fileRev} stack={fileStack} setStack={setStack} />
    </div>
  );
}

/** Select a commit range to split. */
function StackRangeSelector() {
  const stackEdit = useStackEditState();

  const {commitStack} = stackEdit;
  let {splitRange} = stackEdit;
  const [startRev, endRev] = findStartEndRevs(commitStack, splitRange);
  const endKey = (endRev != null && commitStack.get(endRev)?.key) || '';
  splitRange = splitRange.set('endKey', endKey);
  const mutableRevs = commitStack.mutableRevs().reverse();

  const dropdownStyle: React.CSSProperties = {
    width: 'calc(50% - var(--pad))',
    minWidth: 'calc(min(260px, 50vw - 100px))',
    marginBottom: 'var(--pad)',
    zIndex: 3,
  };

  const Dropdown = (props: {isStart: boolean}) => {
    const {isStart} = props;
    const value = isStart ? splitRange.startKey : splitRange.endKey ?? '';
    const id = isStart ? 'split-dropdown-start' : 'split-dropdown-end';
    return (
      <div className="dropdown-container" style={dropdownStyle}>
        <label htmlFor={id}>{isStart ? t('Split start') : t('Split end')}</label>
        <VSCodeDropdown
          id={id}
          value={value}
          disabled={!isStart && startRev == null}
          style={{width: '100%', zIndex: 3}}
          onChange={e => {
            const key = (e.target as unknown as {value: string}).value;
            let newRange = splitRange.set(isStart ? 'startKey' : 'endKey', key);
            if (isStart && endKey === '') {
              newRange = newRange.set('endKey', key);
            }
            stackEdit.setSplitRange(newRange);
          }}>
          <VSCodeOption value="" selected={value === ''}>
            {isStart ? <T>Select split start</T> : <T>Select split end</T>}
          </VSCodeOption>
          <VSCodeDivider />
          {mutableRevs.map(rev => {
            const commit = unwrap(commitStack.get(rev));
            const disabled = isStart ? false : startRev != null && rev < startRev;
            return (
              <VSCodeOption
                key={commit.key}
                value={commit.key}
                disabled={disabled}
                selected={value === commit.key}>
                {commit.text.split('\n', 1)[0]}
              </VSCodeOption>
            );
          })}
        </VSCodeDropdown>
      </div>
    );
  };

  // Intentionally "mistakenly" use "<Dropdown>" instead of "Dropdown()" to force rerendering
  // "<VSCodeDropdown>". This works around an issue that <VSCodeDropdown> has poor support
  // as a "controlled component". For example, if we update the "value" to a new child being
  // rendered, or reorder the "children", VSCodeDropdown might render the "wrong" selected
  // item (ex. based on the index of children, not value of children; or ignore the new
  // "value" if the new child is not yet rendered).
  // See also https://github.com/microsoft/vscode-webview-ui-toolkit/issues/433.
  return (
    <FlexRow>
      <Dropdown isStart={true} />
      <Icon icon="ellipsis" />
      <Dropdown isStart={false} />
    </FlexRow>
  );
}

type MaybeEditableCommitTitleProps = {
  commitMessage: string;
  commitKey?: string;
};

function MaybeEditableCommitTitle(props: MaybeEditableCommitTitleProps) {
  const stackEdit = useStackEditState();

  const {commitMessage, commitKey} = props;

  const isMultiLine = commitMessage.trimEnd().includes('\n');
  if (isMultiLine) {
    return <CommitTitle commitMessage={commitMessage} />;
  } else {
    // Make single-line message (ex. "Split of ....") editable.
    // Don't support multi-line message editing yet due to the complexities
    // of syncing to a code review system.
    const handleEdit = (value?: string) => {
      if (value != null && commitKey != null) {
        const {commitStack} = stackEdit;
        const commit = commitStack.findCommitByKey(commitKey);
        if (commit != null) {
          const newStack = commitStack.stack.setIn([commit.rev, 'text'], value);
          const newCommitStack = commitStack.set('stack', newStack);
          stackEdit.push(newCommitStack, {name: 'metaedit', commit});
        }
      }
    };
    return (
      <VSCodeTextField
        value={commitMessage}
        title={t('Edit commit title')}
        style={{width: 'calc(100% - var(--pad))'}}
        onInput={e => handleEdit((e.target as unknown as {value: string})?.value)}
      />
    );
  }
}

function findStartEndRevs(
  commitStack: CommitStackState,
  splitRange: SplitRangeRecord,
): [Rev | undefined, Rev | undefined] {
  const startRev = commitStack.findCommitByKey(splitRange.startKey)?.rev;
  let endRev = commitStack.findCommitByKey(splitRange.endKey)?.rev;
  if (startRev == null || startRev > (endRev ?? -1)) {
    endRev = undefined;
  }
  return [startRev, endRev];
}

const splitMessagePrefix = t('Split of "');

function getEmptyCommitTitle(commitMessage: string): string {
  let title = '';
  if (!commitMessage.startsWith(splitMessagePrefix)) {
    // foo bar -> Split of "foo bar"
    title = commitMessage.split('\n', 1)[0];
    title = t('Split of "$title"', {replace: {$title: title}});
  } else {
    title = commitMessage.split('\n', 1)[0];
    const sep = t(' #');
    const last = title.split(sep).at(-1) ?? '';
    const number = parseInt(last);
    if (number > 0) {
      // Split of "foo" #2 -> Split of "foo" #3
      title = title.slice(0, -last.length) + (number + 1).toString();
    } else {
      // Split of "foo" -> Split of "foo" #2
      title = title + sep + '2';
    }
  }
  return title;
}

type SplitFileProps = {
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

  /** The rev in the stack to edit. */
  rev: Rev;
};

export function SplitFile(props: SplitFileProps) {
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
  const abBlocks = diffBlocks(aLines, bLines);

  const leftMost = rev <= 1;
  const rightMost = rev + 1 >= stack.revLength;

  const blocks = abBlocks;

  // Collapse unchanged context blocks, preserving the context lines.
  const collapsedBlocks = collapseContextBlocks(blocks, (_aLine, bLine) =>
    expandedLines.has(bLine),
  );

  const lineKind: Array<'add' | 'del' | 'context'> = [];
  const leftGutter: JSX.Element[] = [];
  const leftButtons: JSX.Element[] = [];
  const mainContent: JSX.Element[] = [];
  const rightGutter: JSX.Element[] = [];
  const rightButtons: JSX.Element[] = [];

  const handleContextExpand = (b1: LineIdx, b2: LineIdx) => {
    const newSet = expandedLines.union(Range(b1, b2));
    setExpandedLines(newSet);
  };

  const pushLineButtons = (sign: '=' | '!' | '~', aIdx?: LineIdx, bIdx?: LineIdx) => {
    let leftButton: JSX.Element | null = null;
    let rightButton: JSX.Element | null = null;

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

  const bLineSpan = (bLine: string): JSX.Element => {
    return <span>{bLine}</span>;
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
      lineKind.push('context');
      pushLineButtons(sign, a1, b1);
    } else if (sign === '=') {
      // Unchanged.
      for (let ai = a1; ai < a2; ++ai) {
        const bi = ai + b1 - a1;
        const leftIdx = ai;
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
            {bLineSpan(bLines[bi])}
          </div>,
        );

        lineKind.push('context');
        pushLineButtons(sign, ai, bi);
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
        let className = 'line';
        if (selectedLineIds.has(selId)) {
          className += ' selected';
        }

        pushLineButtons(sign, ai, undefined);
        mainContent.push(
          <div key={-ai} className={className} data-sel-id={selId}>
            {aLines[ai]}
          </div>,
        );
        lineKind.push('del');
      }
      for (let bi = b1; bi < b2; ++bi) {
        // Inserted lines show up in unified and side-by-side diffs.
        const leftClassName = 'lineno';
        leftGutter.push(
          <div className={leftClassName} key={`b${bi}`} data-span-id={`${rev}-${bi}l`}>
            {' '}
          </div>,
        );
        const rightClassName = 'lineno';
        rightGutter.push(
          <div className={rightClassName} key={bi} data-span-id={`${rev}-${bi}r`}>
            {bi + 1}
          </div>,
        );
        const selId = `b${bi}`;
        let lineClassName = 'line';
        if (selectedLineIds.has(selId)) {
          lineClassName += ' selected';
        }
        pushLineButtons(sign, undefined, bi);
        mainContent.push(
          <div key={bi} className={lineClassName} data-sel-id={selId}>
            {bLineSpan(bLines[bi])}
          </div>,
        );
        lineKind.push('add');
      }
    }
  });

  const rows = mainContent.map((line, i) => (
    <tr key={i} className={lineKind[i]}>
      <td className="split-left-button">{leftButtons[i]}</td>
      <td className="split-left-lineno">{leftGutter[i]}</td>
      <td className="split-line-content">{line}</td>
      <td className="split-right-lineno">{rightGutter[i]}</td>
      <td className="split-right-button">{rightButtons[i]}</td>
    </tr>
  ));

  return (
    <div className="split-file">
      <table>
        <colgroup>
          <col width={50} /> {/* left arrows */}
          <col width={50} /> {/* before line numbers */}
          <col width={'100%'} /> {/* diff content */}
          <col width={50} /> {/* after line numbers */}
          <col width={50} /> {/* rightarrow  */}
        </colgroup>
        <tbody>{rows}</tbody>
      </table>
    </div>
  );
}
