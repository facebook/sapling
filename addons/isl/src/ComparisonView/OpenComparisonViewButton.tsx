/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {Comparison} from 'shared/Comparison';

import {T, t} from '../i18n';
import {short} from '../utils';
import {currentComparisonMode} from './atoms';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useSetRecoilState} from 'recoil';
import {ComparisonType} from 'shared/Comparison';
import {Icon} from 'shared/Icon';

export function OpenComparisonViewButton({
  comparison,
  buttonText,
  onClick,
}: {
  comparison: Comparison;
  buttonText?: ReactNode;
  onClick?: () => unknown;
}) {
  const isFake =
    comparison.type === ComparisonType.Committed && comparison.hash.startsWith('OPTIMISTIC');
  const setComparisonMode = useSetRecoilState(currentComparisonMode);
  return (
    <VSCodeButton
      data-testid={`open-comparison-view-button-${comparison.type}`}
      appearance="icon"
      disabled={isFake}
      onClick={() => {
        onClick?.();
        setComparisonMode({comparison, visible: true});
      }}>
      <Icon icon="files" slot="start" />
      {isFake ? <T>View Changes</T> : buttonText ?? buttonLabelForComparison(comparison)}
    </VSCodeButton>
  );
}

function buttonLabelForComparison(comparison: Comparison): string {
  switch (comparison.type) {
    case ComparisonType.UncommittedChanges:
      return t('View Changes');
    case ComparisonType.HeadChanges:
      return t('View Head Changes');
    case ComparisonType.StackChanges:
      return t('View Stack Changes');
    case ComparisonType.Committed:
      return t('View Changes in $hash', {replace: {$hash: short(comparison.hash)}});
    case ComparisonType.SinceLastCodeReviewSubmit:
      return t('Compare $hash with remote', {replace: {$hash: short(comparison.hash)}});
  }
}
