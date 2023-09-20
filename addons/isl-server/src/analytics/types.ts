/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {TrackErrorName, TrackEventName} from './eventNames';

type JSONSerializableObject = {[key: string]: JSONSerializable};
type JSONSerializable =
  | JSONSerializableObject
  | Array<JSONSerializable>
  | string
  | number
  | boolean
  | null
  | undefined;

export type TrackResult = {
  parentId: string;
};

/**
 * Data attached to each analytics event which is likely different for every event.
 * This can be overwritten for each event, in the client or server.
 */
export type TrackData = {
  eventName?: TrackEventName;
  /** Timestamp in ms since unix epoch */
  timestamp?: number;
  /** duration of the event in ms */
  duration?: number;
  /** additional fields and custom data for this event */
  extras?: JSONSerializableObject;
  /** string enum describing what category of error this is */
  errorName?: TrackErrorName;
  /** thrown error message */
  errorMessage?: string;
  /** every event gets a unique id */
  id?: string;
  /** id field from another track event, for cross-referencing */
  parentId?: string;
  /** Unique ID for an sl command, also passed to sl itself for analytics correlation */
  operationId?: string;
};

export type TrackDataWithEventName = TrackData & {eventName: TrackEventName};

/**
 * Data attached to each analytics event, which is common among all events for this instance of ISL.
 * This is only known on the server side, where it's cached as part of the Tracker instance.
 */
export type ApplicationInfo = {
  /** vscode, browser, etc */
  platform: string;
  /** platform-specific version string */
  version: string;
  /** unique identifier for this ISL session */
  sessionId: string;
  unixname: string;
  /* Currently mounted repository identifier */
  repo?: string | undefined;
  /* e.g. 'darwin' or 'win32' or 'linux' */
  osType: string;
  /* e.g. 'x64' or 'arm64' */
  osArch: string;
  /* e.g. '21.6.0 */
  osRelease: string;
  hostname: string;
};

/**
 * Combined data attached to analytics events.
 */
export type FullTrackData = ApplicationInfo & TrackData;
