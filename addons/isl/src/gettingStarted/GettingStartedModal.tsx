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
import {useModal} from '../useModal';
import {useAtom} from 'jotai';
import {Suspense, useEffect, useState} from 'react';
import {Icon} from 'shared/Icon';

export const hasShownGettingStarted = localStorageBackedAtom<boolean | null>(
  'isl.has-shown-getting-started',
  null,
);

export function GettingStartedModal() {
  const [hasShownAlready, setHasShown] = useAtom(hasShownGettingStarted);
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
  return <DismissableGettingStartedModal />;
}

function DismissableGettingStartedModal() {
  const [visible, setVisible] = useState(true);
  useCommand('Escape', () => {
    setVisible(false);
  });

  const showModal = useModal();

  useEffect(() => {
    const GettingStartedBugNuxContent = platform.GettingStartedBugNuxContent;
    if (!visible && GettingStartedBugNuxContent) {
      showModal({
        type: 'custom',
        title: '',
        component: ({returnResultAndDismiss}) => (
          <Suspense>
            <GettingStartedBugNuxContent dismiss={() => returnResultAndDismiss(true)} />
          </Suspense>
        ),
      });
    }
  }, [visible, showModal]);

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
