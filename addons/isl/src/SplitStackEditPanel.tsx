/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Mode} from './FileStackEditor';
import type {CommitStackState} from './stackEdit/commitStackState';
import type {FileStackState, Rev} from './stackEdit/fileStackState';
import type {RepoPath} from 'shared/types/common';

import {CommitTitle} from './CommitTitle';
import {FileHeader} from './ComparisonView/SplitDiffView/SplitDiffFileHeader';
import {FlexRow, Row, ScrollX, ScrollY} from './ComponentUtils';
import {EmptyState} from './EmptyState';
import {FileStackEditor} from './FileStackEditor';
import {VSCodeCheckbox} from './VSCodeCheckbox';
import {t, T} from './i18n';
import {SplitRangeRecord, useStackEditState} from './stackEditState';
import {
  VSCodeDivider,
  VSCodeDropdown,
  VSCodeOption,
  VSCodeRadio,
  VSCodeRadioGroup,
  VSCodeTextField,
} from '@vscode/webview-ui-toolkit/react';
import {Range, Seq} from 'immutable';
import {atom, useRecoilState, useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';
import {DiffType} from 'shared/patch/parse';
import {unwrap} from 'shared/utils';

import './VSCodeDropdown.css';
import './SplitStackEditPanel.css';

const splitEditModeAtom = atom<Mode>({
  key: 'splitEditModeAtom',
  default: 'unified-diff',
});
const splitTextEditAtom = atom<boolean>({
  key: 'splitTextEditAtom',
  default: false,
});

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
    .slice(0, -1 /* Drop the last "empty" commit */)
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
      <EditModeSelector />
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
    <EmptyState>
      <T>This commit is empty.</T>
    </EmptyState>
  ) : (
    <ScrollY maxSize="calc(100vh - 350px)" hideBar={true}>
      {editors}
    </ScrollY>
  );

  // The min width ensures it does not look too narrow for an empty commit.
  return (
    <div className="split-commit-column" style={{minWidth: 300, flexShrink: 0}}>
      <div>
        {rev + 1} / {subStack.size - 1}
      </div>
      <MaybeEditableCommitTitle commitMessage={commitMessage} commitKey={commit?.key} />
      <div style={{marginTop: 'var(--pad)'}} />
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
  const mode = useRecoilValue(splitEditModeAtom);
  const textEdit = useRecoilValue(splitTextEditAtom) || mode === 'side-by-side-diff';

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
      <FileHeader path={path} diffType={DiffType.Modified} />
      <FileStackEditor
        key={fileIdx}
        rev={fileRev}
        stack={fileStack}
        mode={mode}
        textEdit={textEdit}
        setStack={setStack}
      />
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

function EditModeSelector() {
  const [mode, setMode] = useRecoilState(splitEditModeAtom);
  const [textEdit, setTextEdit] = useRecoilState(splitTextEditAtom);

  const handleModeChange = ((e: Event) => {
    setMode((e.target as HTMLInputElement).value as Mode);
  }) as ((e: Event) => unknown) & React.FormEventHandler<HTMLElement>;

  // Blockers of 'side-by-side-diff':
  // - Ribbons are clipped by <ScrollY> (difficult).
  // - `data-span-id` needs a prefix for different paths/files (fixable).
  // - Need to show immutable fileRev 0 to compare against (fixable).
  const showModeSwitch = false;

  return (
    <Row style={{marginTop: 'var(--pad)'}}>
      {showModeSwitch && (
        <VSCodeRadioGroup value={mode} onChange={handleModeChange}>
          <VSCodeRadio accessKey="u" value="unified-diff">
            <T>Unified diff</T>
          </VSCodeRadio>
          <VSCodeRadio accessKey="s" value="side-by-side-diff">
            <T>Side-by-side diff</T>
          </VSCodeRadio>
        </VSCodeRadioGroup>
      )}
      <VSCodeCheckbox
        accessKey="t"
        checked={textEdit || mode === 'side-by-side-diff'}
        disabled={mode === 'side-by-side-diff'}
        onChange={() => {
          setTextEdit(c => !c);
        }}>
        <T>Edit text (advanced)</T>
      </VSCodeCheckbox>
    </Row>
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
