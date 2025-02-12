/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Suspense, lazy} from 'react';
import {T} from '../i18n';
import {useModal} from '../useModal';

const ComponentExplorer = lazy(() => import('isl-components/explorer/ComponentExplorer'));

export function ComponentExplorerButton({dismiss}: {dismiss: () => unknown}) {
  const showModal = useModal();
  return (
    <Button
      onClick={() => {
        dismiss();
        showModal({
          maxWidth: 'calc(min(90vw, 1200px)',
          maxHeight: 'calc(min(90vw, 800px)',
          width: 'inherit',
          height: 'inherit',
          type: 'custom',
          dataTestId: 'component-explorer',
          component: () => (
            <Suspense fallback={<Icon icon="loading" size="M" />}>
              <ComponentExplorer />
            </Suspense>
          ),
        });
      }}>
      <T>Open Component Explorer</T>
    </Button>
  );
}
