/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TrackDataWithEventName} from 'isl-server/src/analytics/types';

import {Tracker} from 'isl-server/src/analytics/tracker';
// @fb-only: import serverAPI from '../ClientToServerAPI';

/** Client-side global analytics tracker */
export const tracker = new Tracker(sendDataToServer, {});

/**
 * The client side sends data to the server-side to actually get tracked.
 *
 * This is inlined (rather than imported from `Internal`) so this low-level module
 * doesn't pull in the `Internal` graph, which would form an import cycle
 * (analytics -> Internal -> InternalImports -> ... -> analytics). The send is
 * `@fb-only`, so in open source it's stripped to a no-op (we don't track there).
 */
// prettier-ignore
function sendDataToServer(data: TrackDataWithEventName) {
  // @fb-only: serverAPI.postMessage({type: 'track', data});
}
