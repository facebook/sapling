/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Rev} from './stackEdit/fileStackState';

import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {T, t} from './i18n';
import {useStackEditState} from './stackEditState';
import {useModal} from './useModal';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {lazy, Suspense} from 'react';
import {Icon} from 'shared/Icon';

const FileStackEditModal = lazy(() => import('./FileStackEditModal'));

export function FileStackEditButton(): React.ReactElement {
  const stackEdit = useStackEditState();
  const showModal = useModal();

  const handleEditFile = () => {
    const title = t('Editing file');
    showModal({
      type: 'custom',
      component: () => {
        return (
          <Suspense>
            <FileStackEditModal />
          </Suspense>
        );
      },
      title,
    });
  };

  return (
    <Tooltip
      title={t('Edit all versions of a file in the stack')}
      delayMs={DOCUMENTATION_DELAY}
      placement="bottom">
      <VSCodeButton appearance="secondary" onClick={() => handleEditFile()}>
        <Icon icon="files" slot="start" />
        <T>Edit file stack</T>
      </VSCodeButton>
    </Tooltip>
  );
}
