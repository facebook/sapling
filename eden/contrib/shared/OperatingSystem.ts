/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

const userAgent = typeof navigator === 'object' ? navigator.userAgent : '';

export const isWindows = userAgent.indexOf('Windows') >= 0;
export const isMac = userAgent.indexOf('Macintosh') >= 0;
