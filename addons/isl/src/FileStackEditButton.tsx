/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {useStackEditState} from './stackEditState';
import {useModal} from './useModal';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';

export function FileStackEditButton(): React.ReactElement {
  const stackEdit = useStackEditState();
  const showModal = useModal();
  const handleEditFile = (label: string, _index: number) => {
    const title = t('Editing $name', {replace: {$name: label}});
    showModal({
      type: 'custom',
      component: ({returnResultAndDismiss: _}) => {
        return <div id="use-modal-message">TODO</div>;
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
