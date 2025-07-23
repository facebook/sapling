/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {Comparison} from 'shared/Comparison';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {ComparisonType} from 'shared/Comparison';
import {T, t} from '../i18n';
import {short} from '../utils';
import {showComparison} from './atoms';

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
  return (
    <Button
      data-testid={`open-comparison-view-button-${comparison.type}`}
      icon
      disabled={isFake}
      onClick={() => {
        onClick?.();
        showComparison(comparison);
      }}>
      <Icon icon="files" slot="start" />
      {isFake ? <T>View Changes</T> : (buttonText ?? buttonLabelForComparison(comparison))}
    </Button>
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
