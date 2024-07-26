/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ComparisonMode} from './atoms';
import type {Comparison} from 'shared/Comparison';

import {useCommand} from '../ISLShortcuts';
import {Modal} from '../Modal';
import {writeAtom} from '../jotaiUtils';
import {currentComparisonMode} from './atoms';
import {Icon} from 'isl-components/Icon';
import {useAtom} from 'jotai';
import {lazy, Suspense} from 'react';
import {ComparisonType} from 'shared/Comparison';

import './ComparisonView.css';

const ComparisonView = lazy(() => import('./ComparisonView'));

function useComparisonView(): ComparisonMode {
  const [mode, setMode] = useAtom(currentComparisonMode);

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
        <ComparisonView
          comparison={mode.comparison}
          dismiss={() =>
            writeAtom(currentComparisonMode, previous => ({...previous, visible: false}))
          }
        />
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
    <Suspense fallback={<Icon icon="loading" />}>
      <ComparisonView comparison={mode.comparison} />
    </Suspense>
  );
}
