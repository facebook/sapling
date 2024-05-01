/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

export function processTerminalLines(segments: Array<string>): Array<string> {
  const result = [];
  // Normalize output buffering to be newline based.
  // This avoids weirdness with output buffering at \r instead of \n, and makes this logic simpler.
  for (const line of segments.join('').split('\n')) {
    const cr = line.lastIndexOf(
      '\r',
      line.length - 2, // ignore \r at the end, that should be handled as \n
    );
    if (cr !== -1) {
      // if there's one or more carriage returns, take the output after the last one as this line.
      result.push(line.slice(cr + 1).trimEnd());
      continue;
    }

    result.push(line.trimEnd());
  }

  while (result.length > 0 && result.at(-1)?.trim() === '') {
    result.pop();
  }

  return result;
}
