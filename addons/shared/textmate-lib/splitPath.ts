/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Split a path into [dirname, basename].
 */
export default function splitPath(path: string): [string, string] {
  const index = path.lastIndexOf('/');
  if (index !== -1) {
    return [path.slice(0, index), path.slice(index + 1)];
  } else {
    return ['', path];
  }
}
