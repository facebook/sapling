/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {atom} from 'jotai';
import {configBackedAtom} from '../jotaiUtils';

/**
 * Atom for storing whether diffs should be submitted as drafts.
 * When true, the `--draft` flag is passed to submit commands.
 * Backed by user config 'isl.submitAsDraft'.
 *
 * Note: When this is set to false, publishWhenReady is also set to false
 * (since publish-when-ready requires draft mode).
 */
const submitAsDraftRaw = configBackedAtom<boolean>('isl.submitAsDraft', false);

export const submitAsDraft = atom(
  get => get(submitAsDraftRaw),
  (_get, set, update: boolean | ((prev: boolean) => boolean)) => {
    const newValue = typeof update === 'function' ? update(_get(submitAsDraftRaw)) : update;
    set(submitAsDraftRaw, newValue);
    // Auto-disable publishWhenReady when draft is disabled
    if (!newValue) {
      set(publishWhenReadyRaw, false);
    }
  },
);

/**
 * Raw atom for storing whether diffs should be published when ready.
 * This is the underlying storage atom; use publishWhenReady for the derived version
 * that enforces the constraint that draft mode must be enabled.
 */
const publishWhenReadyRaw = configBackedAtom<boolean>('isl.publishWhenReady', false);

/**
 * Atom for storing whether diffs should be published when ready.
 * When true, the `--publish-when-ready` flag is passed to submit commands,
 * which triggers CI validation immediately on draft diffs and auto-publishes
 * when all signals pass.
 *
 * This atom enforces the constraint that publishWhenReady requires submitAsDraft:
 * - Reading: Returns false if submitAsDraft is false (even if raw value is true)
 * - Writing: When set to true, also enables submitAsDraft
 */
export const publishWhenReady = atom(
  get => get(submitAsDraftRaw) && get(publishWhenReadyRaw),
  (_get, set, update: boolean | ((prev: boolean) => boolean)) => {
    const currentValue = _get(submitAsDraftRaw) && _get(publishWhenReadyRaw);
    const newValue = typeof update === 'function' ? update(currentValue) : update;
    set(publishWhenReadyRaw, newValue);
    // Auto-enable draft when publishWhenReady is enabled
    if (newValue) {
      set(submitAsDraftRaw, true);
    }
  },
);
