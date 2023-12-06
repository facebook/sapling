/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Get the current UI zoom level, from the --zoom CSS variable.
 * This is NOT the browser zoom level, that does not need to be accounted for.
 * This is the UI setting zoom which must be used in width/height computations
 * instead of e.g. 100vw directly.
 */
export function getZoomLevel(): number {
  try {
    return parseFloat(document.body.style.getPropertyValue('--zoom'));
  } catch {}
  return 1;
}
