/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useCommand} from '../ISLShortcuts';
import {Modal} from '../Modal';
import {persistAtomToConfigEffect} from '../persistAtomToConfigEffect';
import platform from '../platform';
import {Suspense, useEffect, useState} from 'react';
import {atom, useRecoilState} from 'recoil';
import {Icon} from 'shared/Icon';

export const hasShownGettingStarted = atom<boolean | null>({
  key: 'hasShownGettingStarted',
  default: null,
  effects: [persistAtomToConfigEffect<boolean | null>('isl.hasShownGettingStarted', false)],
});

export function GettingStartedModal() {
  const [hasShownAlready, setHasShown] = useRecoilState(hasShownGettingStarted);
  const [isShowingStable, setIsShowingStable] = useState(false);

  useEffect(() => {
    if (hasShownAlready === false) {
      setIsShowingStable(true);
      setHasShown(true);
    }
  }, [hasShownAlready, setHasShown]);
  if (!isShowingStable) {
    return null;
  }
  return <DismissableModal />;
}

function DismissableModal() {
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
    <Modal className="getting-started-modal">
      <Suspense fallback={<Icon icon="loading" />}>
        <ContentComponent dismiss={() => setVisible(false)} />
      </Suspense>
    </Modal>
  );
}
