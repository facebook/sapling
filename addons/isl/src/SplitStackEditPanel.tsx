/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CommitStackState} from './stackEdit/commitStackState';
import type {FileStackState, Rev} from './stackEdit/fileStackState';
import type {RepoPath} from 'shared/types/common';

import {FileHeader} from './ComparisonView/SplitDiffView/SplitDiffFileHeader';
import {useTokenizedContentsOnceVisible} from './ComparisonView/SplitDiffView/syntaxHighlighting';
import {Column, FlexRow, Row, ScrollX, ScrollY} from './ComponentUtils';
import {EmptyState} from './EmptyState';
import {computeLinesForFileStackEditor} from './FileStackEditorLines';
import {Subtle} from './Subtle';
import {Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {SplitRangeRecord, useStackEditState} from './stackEditState';
import {firstLine} from './utils';
import {
  VSCodeButton,
  VSCodeDivider,
  VSCodeDropdown,
  VSCodeOption,
  VSCodeTextField,
} from '@vscode/webview-ui-toolkit/react';
import {Set as ImSet, Range} from 'immutable';
import {useRef, useState, useEffect, useMemo} from 'react';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {type LineIdx, splitLines, diffBlocks} from 'shared/diff';
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
        <EmptyState small>
          <T>Select a commit to split its changes.</T>
          <br />
          <Subtle>
            <T>Or, select a range of commits to move contents among them.</T>
          </Subtle>
        </EmptyState>
      </div>
    );
  }

  // Prepare a "dense" subStack with an extra empty commit to move right.
  const emptyTitle = getEmptyCommitTitle(commitStack.get(endRev)?.text ?? '');
  const subStack = commitStack
    .insertEmpty(endRev + 1, emptyTitle, endRev)
    .denseSubStack(Range(startRev, endRev + 2).toList());

  const insertBlankCommit = (rev: Rev) => {
    const newStack = stackEdit.commitStack.insertEmpty(startRev + rev, t('New Commit'));

    let {splitRange} = stackEdit;
    if (rev === 0) {
      const newStart = newStack.get(startRev);
      if (newStart != null) {
        splitRange = splitRange.set('startKey', newStart.key);
      }
    }

    stackEdit.push(newStack, {name: 'insertBlankCommit'}, splitRange);
  };

  // One commit per column.
  const columns: JSX.Element[] = subStack
    .revs()
    .map(rev => (
      <SplitColumn key={rev} rev={rev} subStack={subStack} insertBlankCommit={insertBlankCommit} />
    ));

  return (
    <div className="interactive-split">
      <ScrollX maxSize="calc(100vw - 50px)">
        <Row style={{padding: '0 var(--pad)'}}>{columns}</Row>
      </ScrollX>
    </div>
  );
}

type SplitColumnProps = {
  subStack: CommitStackState;
  rev: Rev;
  insertBlankCommit: (rev: Rev) => unknown;
};

function InsertBlankCommitButton({
  beforeRev,
  onClick,
}: {
  beforeRev: Rev | undefined;
  onClick: () => unknown;
}) {
  return (
    <div className="split-insert-blank-commit-container" role="button" onClick={onClick}>
      <Tooltip
        placement="top"
        title={
          beforeRev == 0
            ? t('Insert a new blank commit before the next commit')
            : t('Insert a new blank commit between these commits')
        }>
        <div className="split-insert-blank-commit">
          <Icon icon="add" />
        </div>
      </Tooltip>
    </div>
  );
}

function SplitColumn(props: SplitColumnProps) {
  const {subStack, rev, insertBlankCommit} = props;

  const [collapsedFiles, setCollapsedFiles] = useState(new Set());

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
        collapsed={collapsedFiles.has(path)}
        toggleCollapsed={() => {
          const updated = new Set(collapsedFiles);
          updated.has(path) ? updated.delete(path) : updated.add(path);
          setCollapsedFiles(updated);
        }}
      />
    );
    const result = isModified ? [editor] : [];
    return result;
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
    <ScrollY maxSize="calc(100vh - 280px)" hideBar={true}>
      {editors}
    </ScrollY>
  );

  const showExtraCommitActionsContextMenu = useContextMenu(() => {
    const options = [];
    const allFiles = new Set(sortedFileStacks.map(([path]) => path));
    if (collapsedFiles.size < allFiles.size && allFiles.size > 0) {
      options.push({
        label: t('Collapse all files'),
        onClick() {
          setCollapsedFiles(allFiles);
        },
      });
    }
    if (collapsedFiles.size > 0) {
      options.push({
        label: t('Expand all files'),
        onClick() {
          setCollapsedFiles(new Set());
        },
      });
    }
    return options;
  });

  return (
    <>
      {editors.isEmpty() ? null : (
        <InsertBlankCommitButton beforeRev={rev} onClick={() => insertBlankCommit(rev)} />
      )}
      <div className="split-commit-column">
        <div className="split-commit-header">
          <span className="split-commit-header-stack-number">
            {rev + 1} / {subStack.size}
          </span>
          <EditableCommitTitle commitMessage={commitMessage} commitKey={commit?.key} />
          <VSCodeButton appearance="icon" onClick={e => showExtraCommitActionsContextMenu(e)}>
            <Icon icon="ellipsis" />
          </VSCodeButton>
        </div>
        {body}
      </div>
    </>
  );
}

type SplitEditorWithTitleProps = {
  subStack: CommitStackState;
  path: RepoPath;
  fileStack: FileStackState;
  fileIdx: number;
  fileRev: Rev;
  collapsed: boolean;
  toggleCollapsed: () => unknown;
};

function SplitEditorWithTitle(props: SplitEditorWithTitleProps) {
  const stackEdit = useStackEditState();

  const {commitStack} = stackEdit;
  const {subStack, path, fileStack, fileIdx, fileRev, collapsed, toggleCollapsed} = props;

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

  const moveEntireFile = (dir: 'left' | 'right') => {
    const aRev = fileRev - 1;
    const bRev = fileRev;

    const newFileStack = fileStack.mapAllLines(line => {
      let newRevs = line.revs;
      const inA = line.revs.has(aRev);
      const inB = line.revs.has(bRev);
      const isContext = inA && inB;
      if (!isContext) {
        if (inA) {
          // This is a deletion.
          if (dir === 'right') {
            // Move deletion right - add it in bRev.
            newRevs = newRevs.add(bRev);
          } else {
            // Move deletion left - drop it from aRev.
            newRevs = newRevs.remove(aRev);
          }
        }
        if (inB) {
          // This is an insertion.
          if (dir === 'right') {
            // Move insertion right - drop it in bRev.
            newRevs = newRevs.remove(bRev);
          } else {
            // Move insertion left - add it to aRev.
            newRevs = newRevs.add(aRev);
          }
        }
      }
      return newRevs === line.revs ? line : line.set('revs', newRevs);
    });

    setStack(newFileStack);
  };

  return (
    <div className="split-commit-file">
      <FileHeader
        path={path}
        diffType={DiffType.Modified}
        open={!collapsed}
        onChangeOpen={toggleCollapsed}
        fileActions={
          <div className="split-commit-file-arrows">
            {fileRev > 1 /* rev == 0 corresponds to fileRev == 1  */ ? (
              <VSCodeButton appearance="icon" onClick={() => moveEntireFile('left')}>
                ⬅
              </VSCodeButton>
            ) : null}
            <VSCodeButton appearance="icon" onClick={() => moveEntireFile('right')}>
              ⮕
            </VSCodeButton>
          </div>
        }
      />
      {!collapsed && (
        <SplitFile key={fileIdx} rev={fileRev} stack={fileStack} setStack={setStack} path={path} />
      )}
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
        <label htmlFor={id} className="split-range-label">
          {isStart ? t('Split start') : t('Split end')}
        </label>
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
    <FlexRow className="split-range-selector">
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

function EditableCommitTitle(props: MaybeEditableCommitTitleProps) {
  const stackEdit = useStackEditState();

  const {commitMessage, commitKey} = props;

  const existingTitle = firstLine(commitMessage);
  const existingDescription = commitMessage.slice(existingTitle.length + 1);

  // Only allow changing the commit title, not the rest of the commit message.
  const handleEdit = (newTitle?: string) => {
    if (newTitle != null && commitKey != null) {
      const {commitStack} = stackEdit;
      const commit = commitStack.findCommitByKey(commitKey);
      if (commit != null) {
        const newFullText = newTitle + '\n' + existingDescription;
        const newStack = commitStack.stack.setIn([commit.rev, 'text'], newFullText);
        const newCommitStack = commitStack.set('stack', newStack);
        stackEdit.push(newCommitStack, {name: 'metaedit', commit});
      }
    }
  };
  return (
    <VSCodeTextField
      value={existingTitle}
      title={t('Edit commit title')}
      style={{width: 'calc(100% - var(--pad))'}}
      onInput={e => handleEdit((e.target as unknown as {value: string})?.value)}
    />
  );
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

  /** The filepath */
  path: string;
};

export function SplitFile(props: SplitFileProps) {
  const mainContentRef = useRef<HTMLTableElement | null>(null);
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
  const aText = stack.getRev(Math.max(0, rev - 1));
  // memo to avoid syntax highlighting repeatedly even when the text hasn't changed
  const bLines = useMemo(() => splitLines(bText), [bText]);
  const aLines = useMemo(() => splitLines(aText), [aText]);
  const abBlocks = diffBlocks(aLines, bLines);

  const highlights = useTokenizedContentsOnceVisible(props.path, aLines, bLines, mainContentRef);

  const {leftGutter, leftButtons, mainContent, rightGutter, rightButtons, lineKind} =
    computeLinesForFileStackEditor(
      stack,
      setStack,
      rev,
      'unified-diff',
      aLines,
      bLines,
      highlights?.[0],
      highlights?.[1],
      abBlocks,
      [],
      abBlocks,
      expandedLines,
      setExpandedLines,
      selectedLineIds,
      [],
      false,
      false,
    );

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
      <table ref={mainContentRef}>
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

export function SplitStackToolbar() {
  return <StackRangeSelector />;
}
