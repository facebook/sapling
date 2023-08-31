/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Mode} from './FileStackEditor';
import type {Rev} from './stackEdit/fileStackState';

import {Row} from './ComponentUtils';
import {FileStackEditorRow} from './FileStackEditor';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {VSCodeCheckbox} from './VSCodeCheckbox';
import {T, t} from './i18n';
import {FileStackState} from './stackEdit/fileStackState';
import {bumpStackEditMetric, useStackEditState} from './stackEditState';
import {useModal} from './useModal';
import {VSCodeButton, VSCodeRadio, VSCodeRadioGroup} from '@vscode/webview-ui-toolkit/react';
import {useState} from 'react';
import {atom, useRecoilState, useSetRecoilState} from 'recoil';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {unwrap} from 'shared/utils';

const fileStackAtom = atom<FileStackState>({
  key: 'fileStackAtom',
  default: new FileStackState([]),
});

export function FileStackEditButton(): React.ReactElement {
  const stackEdit = useStackEditState();
  const setFileStack = useSetRecoilState(fileStackAtom);
  const showModal = useModal();

  const handleEditFile = (label: string, fileIdx: number) => {
    const title = t('Editing $name', {replace: {$name: label}});
    const stack = unwrap(stackEdit.commitStack.fileStacks.get(fileIdx));
    setFileStack(stack);
    showModal({
      type: 'custom',
      component: ({returnResultAndDismiss: close}) => {
        const getTitle = (rev: Rev) =>
          stackEdit.commitStack.getCommitFromFileStackRev(fileIdx, rev)?.text ??
          t(
            '(Base version)\n\n' +
              'Not part of the stack being edited. ' +
              'Cannot be edited here.\n\n' +
              'Provided to show diff against changes in the stack.',
          );
        const skip = (rev: Rev) => stackEdit.commitStack.isAbsentFromFileStackRev(fileIdx, rev);
        return (
          <FileStackEditModalContent
            getTitle={getTitle}
            skip={skip}
            close={close}
            fileIdx={fileIdx}
            fileDesc={label}
          />
        );
      },
      title,
    });
  };

  const showFileStackMenu = useContextMenu(() => {
    const stack = stackEdit.commitStack;
    return stack.fileStacks
      .map((_f, i) => {
        const label = stack.getFileStackDescription(i);
        return {
          label,
          onClick: () => handleEditFile(label, i),
        };
      })
      .toArray();
  });

  return (
    <Tooltip
      title={t('Edit all versions of a file in the stack')}
      delayMs={DOCUMENTATION_DELAY}
      placement="bottom">
      <VSCodeButton appearance="secondary" onClick={showFileStackMenu}>
        <Icon icon="files" slot="start" />
        <T>Edit file stack</T>
      </VSCodeButton>
    </Tooltip>
  );
}

const editModeAtom = atom<Mode>({
  key: 'editModeAtom',
  default: 'unified-diff',
});

function FileStackEditModalContent(props: {
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
            <T>Unified stack</T>
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
