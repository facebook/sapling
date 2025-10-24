/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {ButtonDropdown} from 'isl-components/ButtonDropdown';
import {Icon} from 'isl-components/Icon';
import {useFeatureFlagSync} from './featureFlags';
import {Internal} from './Internal';

export function SmartActionsDropdown() {
  const smartActionsMenuEnabled = useFeatureFlagSync(Internal.featureFlags?.SmartActionsMenu);

  if (!smartActionsMenuEnabled) {
    return null;
  }

  return (
    <ButtonDropdown
      kind="icon"
      options={[
        {id: 'primary-action', label: 'Primary action'},
        {id: 'other-action', label: 'Other action'},
      ]}
      selected={{id: 'primary-action', label: 'Primary action'}}
      icon={<Icon icon="lightbulb-sparkle" />}
      onClick={() => {}}
      onChangeSelected={() => {}}
    />
  );
}
