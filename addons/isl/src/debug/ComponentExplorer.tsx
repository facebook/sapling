/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {T} from '../i18n';
import {useModal} from '../useModal';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {Suspense, lazy} from 'react';
import {Icon} from 'shared/Icon';

const ComponentExplorerModal = lazy(() => import('./ComponentExplorerModal'));

export function ComponentExplorerButton({dismiss}: {dismiss: () => unknown}) {
  const showModal = useModal();
  return (
    <VSCodeButton
      onClick={() => {
        dismiss();
        showModal({
          maxWidth: 'calc(min(90vw, 1200px)',
          maxHeight: 'calc(min(90vw, 800px)',
          width: 'inherit',
          height: 'inherit',
          type: 'custom',
          dataTestId: 'component-explorer',
          component: ({returnResultAndDismiss}) => (
            <Suspense fallback={<Icon icon="loading" size="M" />}>
              <ComponentExplorerModal dismiss={returnResultAndDismiss} />
            </Suspense>
          ),
        });
      }}
      appearance="secondary">
      <T>Open Component Explorer</T>
    </VSCodeButton>
  );
}
