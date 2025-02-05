/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// @fb-only

/* eslint-disable @typescript-eslint/no-explicit-any */

// This file contains imports only used by non-OSS internal builds of ISL
// This should be the only file using fb-only imports and prettier ignores.

// prettier-ignore
type InternalImportsType =
  // @fb-only
// @fb-only
  {[key: string]: undefined | any}
// @fb-only

/**
 * API for accessing internal (non-OSS) features / functions.
 * In OSS builds, all properties will give `undefined`.
 */
export const Internal: InternalImportsType = {
  // @fb-only
};
