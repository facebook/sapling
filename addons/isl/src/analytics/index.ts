/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TrackDataWithEventName} from 'isl-server/src/analytics/types';

import {Tracker} from 'isl-server/src/analytics/tracker';
import {Internal} from '../Internal';

/** Client-side global analytics tracker */
export const tracker = new Tracker(sendDataToServer, {});
window.globalIslClientTracker = tracker;

/**
 * The client side sends data to the server-side to actually get tracked.
 */
// prettier-ignore
function sendDataToServer(data: TrackDataWithEventName) {
  // In open source, we don't even need to bother sending these messages to the server,
  // since we don't track anything anyway.
  Internal?.sendAnalyticsDataToServer?.(data);
}
