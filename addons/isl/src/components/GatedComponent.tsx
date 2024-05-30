/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {useFeatureFlagSync} from '../featureFlags';

export default function GatedComponent({
  featureFlag,
  children,
}: {
  children: React.ReactNode;
  featureFlag: string | undefined;
}) {
  const featureEnabled = useFeatureFlagSync(featureFlag);

  if (!featureEnabled) {
    return null;
  }
  return children;
}
