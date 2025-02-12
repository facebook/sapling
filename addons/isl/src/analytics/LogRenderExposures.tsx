/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TrackEventName} from 'isl-server/src/analytics/eventNames';
import type {TrackData} from 'isl-server/src/analytics/types';
import type {ReactNode} from 'react';

import {useThrottledEffect} from 'shared/hooks';
import {tracker} from './index';

/**
 * Log an analytics event when a component renders the first time.
 * Useful for declarative analytics when there isn't a good place to put a useEffect.
 */
export function LogRenderExposures({
  children,
  eventName,
  data,
}: {
  children: ReactNode;
  eventName: TrackEventName;
  data?: TrackData;
}) {
  useThrottledEffect(
    () => {
      tracker.track(eventName, data);
    },
    100,
    [data, eventName],
  );
  return <>{children}</>;
}
