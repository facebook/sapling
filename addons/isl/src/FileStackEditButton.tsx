/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Rev} from './stackEdit/fileStackState';

import {FileStackEditorRow} from './FileStackEditor';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {FileStackState} from './stackEdit/fileStackState';
import {useStackEditState} from './stackEditState';
import {useModal} from './useModal';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
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
      component: ({returnResultAndDismiss: _}) => {
        const getTitle = (rev: Rev) =>
          stackEdit.commitStack.getCommitFromFileStackRev(fileIdx, rev)?.text ??
          t(
            '(Base version)\n\n' +
              'Not part of the stack being edited. ' +
              'Cannot be edited here.\n\n' +
              'Provided to show diff against changes in the stack.',
          );
        const skip = (rev: Rev) => stackEdit.commitStack.isAbsentFromFileStackRev(fileIdx, rev);
        return <FileStackEditModalContent getTitle={getTitle} skip={skip} />;
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

function FileStackEditModalContent(props: {
  getTitle: (rev: Rev) => string;
  skip: (rev: Rev) => boolean;
}) {
  const [stack, setStack] = useRecoilState(fileStackAtom);
  return (
    <FileStackEditorRow
      stack={stack}
      setStack={setStack}
      getTitle={props.getTitle}
      skip={props.skip}
      mode="unified-diff"
    />
  );
}
