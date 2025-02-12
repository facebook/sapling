/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ComparisonMode} from './atoms';

import {Icon} from 'isl-components/Icon';
import {useAtomValue} from 'jotai';
import {lazy, Suspense} from 'react';
import {ComparisonType} from 'shared/Comparison';
import {useCommand} from '../ISLShortcuts';
import {Modal} from '../Modal';
import {currentComparisonMode, dismissComparison, showComparison} from './atoms';

import './ComparisonView.css';

const ComparisonView = lazy(() => import('./ComparisonView'));

function useComparisonView(): ComparisonMode {
  const mode = useAtomValue(currentComparisonMode);

  useCommand('Escape', () => {
    dismissComparison();
  });
  useCommand('OpenUncommittedChangesComparisonView', () => {
    showComparison({type: ComparisonType.UncommittedChanges});
  });
  useCommand('OpenHeadChangesComparisonView', () => {
    showComparison({type: ComparisonType.HeadChanges});
  });

  return mode;
}

export function ComparisonViewModal() {
  const mode = useComparisonView();

  if (!mode.visible) {
    return null;
  }

  return (
    <Modal className="comparison-view-modal" height="" width="">
      <Suspense fallback={<Icon icon="loading" />}>
        <ComparisonView comparison={mode.comparison} dismiss={dismissComparison} />
      </Suspense>
    </Modal>
  );
}

export function ComparisonViewApp() {
  const mode = useComparisonView();

  if (!mode.visible) {
    return null;
  }

  return (
    <div className="comparison-view-root">
      <Suspense fallback={<Icon icon="loading" />}>
        <ComparisonView comparison={mode.comparison} />
      </Suspense>
    </div>
  );
}
