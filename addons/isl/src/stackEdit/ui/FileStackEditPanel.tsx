/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {FileStackState, Rev} from '../fileStackState';
import type {Mode} from './FileStackEditorLines';

import {Row} from '../../ComponentUtils';
import {EmptyState} from '../../EmptyState';
import {VSCodeCheckbox} from '../../VSCodeCheckbox';
import {t, T} from '../../i18n';
import {FileStackEditorRow} from './FileStackEditor';
import {bumpStackEditMetric, useStackEditState} from './stackEditState';
import {
  VSCodeDivider,
  VSCodeDropdown,
  VSCodeOption,
  VSCodeRadio,
  VSCodeRadioGroup,
} from '@vscode/webview-ui-toolkit/react';
import {useState} from 'react';
import {atom, useRecoilState} from 'recoil';
import {unwrap} from 'shared/utils';

import './VSCodeDropdown.css';

const editModeAtom = atom<Mode>({
  key: 'editModeAtom',
  default: 'unified-diff',
});

export default function FileStackEditPanel() {
  const [fileIdx, setFileIdx] = useState<null | number>(null);
  const [mode, setMode] = useRecoilState(editModeAtom);
  const [textEdit, setTextEdit] = useState(false);
  const stackEdit = useStackEditState();

  // VSCode toolkit does not provide a way to proper type `e`.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const handleModeChange = (e: any) => {
    setMode(e.target.value);
  };

  // File list dropdown.
  const commitStack = stackEdit.commitStack.maybeBuildFileStacks();
  const pathFileIdxList: Array<[string, number]> = commitStack.fileStacks
    .map((_f, i): [string, number] => {
      const label = commitStack.getFileStackDescription(i);
      return [label, i];
    })
    .toArray()
    .sort();
  const fileSelector = (
    <div
      className="dropdown-container"
      style={{
        marginBottom: 'var(--pad)',
        width: '100%',
        minWidth: '500px',
        zIndex: 3,
      }}>
      <label htmlFor="stack-file-dropdown">File to edit</label>
      <VSCodeDropdown
        id="stack-file-dropdown"
        value={fileIdx == null ? 'none' : fileIdx.toString()}
        style={{width: '100%', zIndex: 3}}
        onChange={e => {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          const idx = (e.target as any).value;
          setFileIdx(idx === 'none' ? null : parseInt(idx));
        }}>
        <VSCodeOption value="none">
          <T>Select a file to edit</T>
        </VSCodeOption>
        <VSCodeDivider />
        {pathFileIdxList.map(([path, idx]) => (
          <VSCodeOption key={idx} value={idx.toString()}>
            {path}
          </VSCodeOption>
        ))}
      </VSCodeDropdown>
    </div>
  );

  if (fileIdx == null) {
    return (
      <div>
        {fileSelector}
        <EmptyState small>
          <T>Select a file to see all changes in a row.</T>
        </EmptyState>
      </div>
    );
  }

  // Properties for file stack editing.
  const stack = unwrap(commitStack.fileStacks.get(fileIdx));
  const getTitle = (rev: Rev) =>
    commitStack.getCommitFromFileStackRev(fileIdx, rev)?.text ??
    t(
      '(Base version)\n\n' +
        'Not part of the stack being edited. ' +
        'Cannot be edited here.\n\n' +
        'Provided to show diff against changes in the stack.',
    );
  const skip = (rev: Rev) => commitStack.isAbsentFromFileStackRev(fileIdx, rev);
  const setStack = (newStack: FileStackState) => {
    const fileDesc = commitStack.getFileStackDescription(fileIdx);
    const newCommitStack = commitStack.setFileStack(fileIdx, newStack);
    stackEdit.push(newCommitStack, {name: 'fileStack', fileDesc});
    bumpStackEditMetric('fileStackEdit');
  };

  const editorRow = (
    <FileStackEditorRow
      stack={stack}
      setStack={setStack}
      getTitle={getTitle}
      skip={skip}
      mode={mode}
      textEdit={textEdit || mode === 'side-by-side-diff'}
    />
  );

  return (
    <div>
      {fileSelector}
      <div
        style={{
          marginLeft: 'calc(0px - var(--pad))',
          marginRight: 'calc(0px - var(--pad))',
          minWidth: 'calc((100vw / var(--zoom)) - 81px)',
          minHeight: 'calc((100vh / var(--zoom)) - 265px)',
        }}>
        {editorRow}
      </div>
      <Row>
        <VSCodeRadioGroup value={mode} onChange={handleModeChange}>
          <VSCodeRadio accessKey="u" value="unified-diff">
            <T>Unified diff</T>
          </VSCodeRadio>
          <VSCodeRadio accessKey="s" value="side-by-side-diff">
            <T>Side-by-side diff</T>
          </VSCodeRadio>
          <VSCodeRadio value="unified-stack">
            <T>Unified stack (advanced)</T>
          </VSCodeRadio>
        </VSCodeRadioGroup>
        <VSCodeCheckbox
          accessKey="t"
          checked={textEdit || mode === 'side-by-side-diff'}
          disabled={mode === 'side-by-side-diff'}
          onChange={() => {
            setTextEdit(c => !c);
          }}>
          <T>Edit text</T>
        </VSCodeCheckbox>
      </Row>
    </div>
  );
}
