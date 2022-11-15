/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Comparison} from 'shared/Comparison';

import {useCommand} from '../ISLShortcuts';
import {Icon} from '../Icon';
import {Modal} from '../Modal';
import {currentComparisonMode} from './atoms';
import {lazy, Suspense} from 'react';
import './ComparisonView.css';
import {useRecoilState} from 'recoil';
import {ComparisonType} from 'shared/Comparison';

const ComparisonView = lazy(() => import('./ComparisonView'));

export function ComparisonViewModal() {
  const [mode, setMode] = useRecoilState(currentComparisonMode);

  function toggle(newComparison: Comparison) {
    setMode(lastMode =>
      lastMode.comparison === newComparison
        ? // If the comparison mode hasn't changed, then we want to toggle the view visibility.
          {visible: !mode.visible, comparison: newComparison}
        : // If the comparison changed, then force it to open, regardless of if it was open before.
          {visible: true, comparison: newComparison},
    );
  }

  useCommand('Escape', () => {
    setMode(mode => ({...mode, visible: false}));
  });
  useCommand('OpenUncommittedChangesComparisonView', () => {
    toggle({type: ComparisonType.UncommittedChanges});
  });
  useCommand('OpenHeadChangesComparisonView', () => {
    toggle({type: ComparisonType.HeadChanges});
  });

  if (!mode.visible) {
    return null;
  }

  return (
    <Modal className="comparison-view-modal">
      <Suspense fallback={<Icon icon="loading" />}>
        <ComparisonView comparison={mode.comparison} />
      </Suspense>
    </Modal>
  );
}
