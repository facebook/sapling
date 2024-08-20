/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useCommand} from '../ISLShortcuts';
import {Modal} from '../Modal';
import {localStorageBackedAtom} from '../jotaiUtils';
import platform from '../platform';
import {Icon} from 'isl-components/Icon';
import {useAtom} from 'jotai';
import {Suspense, useEffect, useState} from 'react';

export const hasShownGettingStarted = localStorageBackedAtom<boolean | null>(
  'isl.has-shown-getting-started',
  null,
);

export function GettingStartedModal() {
  const [hasShownAlready, setHasShown] = useAtom(hasShownGettingStarted);
  const [isShowingStable, setIsShowingStable] = useState(false);

  useEffect(() => {
    if (hasShownAlready !== true) {
      setIsShowingStable(true);
      setHasShown(true);
    }
  }, [hasShownAlready, setHasShown]);
  if (!isShowingStable) {
    return null;
  }
  return <DismissableGettingStartedModal />;
}

function DismissableGettingStartedModal() {
  const [visible, setVisible] = useState(true);
  useCommand('Escape', () => {
    setVisible(false);
  });

  if (!visible) {
    return null;
  }

  const ContentComponent = platform.GettingStartedContent;
  if (ContentComponent == null) {
    return null;
  }

  return (
    <Modal className="getting-started-modal" maxHeight={'90vh'}>
      <Suspense fallback={<Icon icon="loading" />}>
        <ContentComponent dismiss={() => setVisible(false)} />
      </Suspense>
    </Modal>
  );
}
