/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export default function sumFileLineChanges(
  files: ReadonlyArray<{additions: number; deletions: number}>,
): {additions: number; deletions: number} {
  return files.reduce(
    (total, file) => ({
      additions: total.additions + file.additions,
      deletions: total.deletions + file.deletions,
    }),
    {additions: 0, deletions: 0},
  );
}
