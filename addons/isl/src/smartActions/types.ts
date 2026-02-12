/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Internal} from '../Internal';
import type {
  CommitInfo,
  MergeConflicts,
  PlatformName,
  PlatformSpecificClientToServerMessages,
} from '../types';

export type PlatformRestriction = Array<PlatformName>;
export type FeatureFlagKey = keyof NonNullable<(typeof Internal)['featureFlags']>;

/** Common properties shared by all action types */
export type SmartActionConfig = {
  id: string;
  label: string;
  description?: string;
  icon?: string;
  trackEventName: string;
  featureFlag?: FeatureFlagKey;
  platformRestriction?: PlatformRestriction;
  shouldShow?: (context: ActionContext) => boolean;
  getMessagePayload: (context: ActionContext) => PlatformSpecificClientToServerMessages;
};

/** Context containing all dependencies actions might need */
export type ActionContext = {
  commit?: CommitInfo;
  repoPath?: string;
  conflicts?: MergeConflicts;
  userContext?: string;
};

export type ActionMenuItem = {
  id: string;
  label: string;
  config: SmartActionConfig;
};
