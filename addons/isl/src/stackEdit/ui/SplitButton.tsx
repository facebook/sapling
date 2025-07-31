/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TrackEventName} from 'isl-server/src/analytics/eventNames';
import type {CommitInfo} from '../../types';

import type {Button} from 'isl-components/Button';
import {T} from '../../i18n';
import {SplitCommitIcon} from '../../icons/SplitCommitIcon';
import {BaseSplitButton} from './BaseSplitButton';

/** Button to open split UI for the current commit. Expected to be shown on the head commit.
 * Loads that one commit in the split UI. */
export function SplitButton({
  commit,
  trackerEventName,
  ...buttonProps
}: {
  commit: CommitInfo;
  trackerEventName: TrackEventName;
} & React.ComponentProps<typeof Button>) {
  return (
    <BaseSplitButton
      commit={commit}
      trackerEventName={trackerEventName}
      bumpSplitFromSuggestion={trackerEventName === 'SplitOpenFromSplitSuggestion'}
      {...buttonProps}>
      <SplitCommitIcon />
      <T>Split</T>
    </BaseSplitButton>
  );
}
