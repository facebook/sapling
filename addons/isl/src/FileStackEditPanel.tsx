/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Mode} from './FileStackEditor';
import type {FileStackState, Rev} from './stackEdit/fileStackState';

import {Row} from './ComponentUtils';
import {FileStackEditorRow} from './FileStackEditor';
import {VSCodeCheckbox} from './VSCodeCheckbox';
import {t, T} from './i18n';
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
  const commitStack = stackEdit.commitStack;
  const pathFileIdxList: Array<[string, number]> = commitStack.fileStacks
    .map((_f, i): [string, number] => {
      const label = commitStack.getFileStackDescription(i);
      return [label, i];
    })
    .toArray()
    .sort();
  const fileSelector = (
    <VSCodeDropdown
      value={fileIdx == null ? 'none' : fileIdx.toString()}
      style={{
        margin: '0 var(--pad)',
        marginBottom: 'var(--pad)',
        width: 'calc(100% - var(--pad) * 2)',
        minWidth: '450px',
        zIndex: 3,
      }}
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
  );

  if (fileIdx == null) {
    return <div>{fileSelector}</div>;
  }

  // Properties for file stack editing.
  const stack = unwrap(stackEdit.commitStack.fileStacks.get(fileIdx));
  const getTitle = (rev: Rev) =>
    stackEdit.commitStack.getCommitFromFileStackRev(fileIdx, rev)?.text ??
    t(
      '(Base version)\n\n' +
        'Not part of the stack being edited. ' +
        'Cannot be edited here.\n\n' +
        'Provided to show diff against changes in the stack.',
    );
  const skip = (rev: Rev) => stackEdit.commitStack.isAbsentFromFileStackRev(fileIdx, rev);
  const setStack = (newStack: FileStackState) => {
    const fileDesc = stackEdit.commitStack.getFileStackDescription(fileIdx);
    const newCommitStack = stackEdit.commitStack.setFileStack(fileIdx, newStack);
    stackEdit.push(newCommitStack, {name: 'fileStack', fileDesc});
    bumpStackEditMetric('fileStackEdit');
  };

  return (
    <div>
      {fileSelector}
      <FileStackEditorRow
        stack={stack}
        setStack={setStack}
        getTitle={getTitle}
        skip={skip}
        mode={mode}
        textEdit={textEdit || mode === 'side-by-side-diff'}
      />
      <Row style={{marginTop: 'var(--pad)'}}>
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
