/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {NUM_LINES_OF_CONTEXT} from './constants';
import {DiffSide} from './generated/graphql';
import {structuredPatch} from 'diff';
import organizeLinesIntoGroups from 'shared/SplitDiffView/organizeLinesIntoGroups';

export type LineToPosition = {[key in DiffSide]: {[key: number]: number}};

/**
 * Returns mapping of line number to "position" in a diff.
 *
 * From the GitHub REST API docs: The position value equals the number of lines
 * down from the first "@@" hunk header in the file you want to add a comment.
 * The line just below the "@@" line is position 1, the next line is position
 * 2, and so on. The position in the diff continues to increase through lines
 * of whitespace and additional hunks until the beginning of a new file.
 *
 * See https://docs.github.com/en/rest/pulls/comments#create-a-review-comment-for-a-pull-request
 *
 * According to the above definition of "position", we can assign positions for
 * each line of the below diff. Additional "@@" hunk header lines also occupy a
 * position. GitHub diffs appear to include NUM_LINES_OF_CONTEXT lines of context.
 *
 *  0 @@ -1,4 +1,4 @@
 *  1 -a
 *  2 +c
 *  3 common
 *  4 common
 *  5 common
 *  6 @@ -6,4 +6,4 @@
 *  7 common
 *  8 common
 *  9 common
 * 10 -b
 * 11 +d
 */
export default function lineToPosition(left: string, right: string): LineToPosition {
  const leftMapping: {[key: number]: number} = {};
  const rightMapping: {[key: number]: number} = {};

  // Because the patch is never returned to the user, the file name does not
  // matter.
  const patch = structuredPatch(
    '' /* oldFileName */,
    '' /* newFileName */,
    left,
    right,
    undefined,
    undefined,
    {
      context: NUM_LINES_OF_CONTEXT,
    },
  );

  let position = 0;
  patch.hunks.forEach(({lines, oldStart, newStart}) => {
    const groups = organizeLinesIntoGroups(lines);
    let leftLine = oldStart;
    let rightLine = newStart;
    ++position;

    groups.forEach(({common, removed, added}) => {
      let count = common.length;
      while (--count >= 0) {
        leftMapping[leftLine] = position;
        rightMapping[rightLine] = position;
        ++leftLine;
        ++rightLine;
        ++position;
      }
      count = removed.length;
      while (--count >= 0) {
        leftMapping[leftLine] = position;
        ++leftLine;
        ++position;
      }
      count = added.length;
      while (--count >= 0) {
        rightMapping[rightLine] = position;
        ++rightLine;
        ++position;
      }
    });
  });

  return {
    [DiffSide.Left]: leftMapping,
    [DiffSide.Right]: rightMapping,
  };
}
