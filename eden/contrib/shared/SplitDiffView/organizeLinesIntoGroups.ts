/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

type HunkGroup = {
  common: string[];
  removed: string[];
  added: string[];
};

/**
 * We must find the groups within `lines` so that multiline sequences of
 * modified lines are displayed correctly. A group is defined by:
 *
 * - a sequence of 0 or more "common lines" that start with ' '
 * - a sequence of 0 or more "removed lines" that start with '-'
 * - a sequence of 0 or more "added lines" that start with '+'
 *
 * Therefore, the end of a group is determined by either:
 *
 * - reaching the end of a list of lines
 * - encountering a "common line" after an "added" or "removed" line.
 */
export default function organizeLinesIntoGroups(lines: string[]): HunkGroup[] {
  const groups = [];
  let group = newGroup();
  lines.forEach(fullLine => {
    const firstChar = fullLine.charAt(0);
    const line = fullLine.slice(1);
    if (firstChar === ' ') {
      if (hasDeltas(group)) {
        // This must be the start of a new group!
        groups.push(group);
        group = newGroup();
      }
      group.common.push(line);
    } else if (firstChar === '-') {
      group.removed.push(line);
    } else if (firstChar === '+') {
      group.added.push(line);
    }
  });

  groups.push(group);

  return groups;
}

function hasDeltas(group: HunkGroup): boolean {
  return group.removed.length !== 0 || group.added.length !== 0;
}

function newGroup(): HunkGroup {
  return {
    common: [],
    removed: [],
    added: [],
  };
}
