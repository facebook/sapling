/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Mode} from './FileStackEditor';
import type {Rev} from './stackEdit/fileStackState';

import {Row} from './ComponentUtils';
import {fileStackAtom} from './FileStackEditButton';
import {FileStackEditorRow} from './FileStackEditor';
import {VSCodeCheckbox} from './VSCodeCheckbox';
import {T} from './i18n';
import {bumpStackEditMetric, useStackEditState} from './stackEditState';
import {VSCodeButton, VSCodeRadio, VSCodeRadioGroup} from '@vscode/webview-ui-toolkit/react';
import {useState} from 'react';
import {atom, useRecoilState} from 'recoil';

const editModeAtom = atom<Mode>({
  key: 'editModeAtom',
  default: 'unified-diff',
});

export default function FileStackEditModal(props: {
  getTitle: (rev: Rev) => string;
  skip: (rev: Rev) => boolean;
  close: (data: unknown) => void;
  fileIdx: number;
  fileDesc: string;
}) {
  const [stack, setStack] = useRecoilState(fileStackAtom);
  const [mode, setMode] = useRecoilState(editModeAtom);
  const [textEdit, setTextEdit] = useState(false);
  const stackEdit = useStackEditState();

  // VSCode toolkit does not provide a way to proper type `e`.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const handleModeChange = (e: any) => {
    setMode(e.target.value);
  };

  const handleConfirm = () => {
    const newCommitStack = stackEdit.commitStack.setFileStack(props.fileIdx, stack);
    stackEdit.push(newCommitStack, {name: 'fileStack', fileDesc: props.fileDesc});
    bumpStackEditMetric('fileStackEdit');
    props.close(true);
  };

  return (
    <div>
      <FileStackEditorRow
        stack={stack}
        setStack={setStack}
        getTitle={props.getTitle}
        skip={props.skip}
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
        <VSCodeButton
          appearance="secondary"
          style={{marginLeft: 'auto'}}
          onClick={() => props.close(false)}>
          <T>Cancel</T>
        </VSCodeButton>
        <VSCodeButton
          appearance="primary"
          style={{marginLeft: 'var(--pad)'}}
          onClick={handleConfirm}>
          <T>Confirm</T>
        </VSCodeButton>
      </Row>
    </div>
  );
}
