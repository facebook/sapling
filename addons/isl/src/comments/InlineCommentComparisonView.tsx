/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Context} from 'isl/src/ComparisonView/SplitDiffView/types';
import type {ParsedDiff} from 'shared/patch/types';

import {SplitDiffTable} from 'isl/src/ComparisonView/SplitDiffView/SplitDiffHunk';

export default function InlineCommentComparisonView({
  ctx,
  path,
  suggestion,
}: {
  ctx: Context;
  path: string;
  suggestion: ParsedDiff;
}) {
  return (
    <div className="split-diff-view">
      <SplitDiffTable ctx={ctx} path={path} patch={suggestion} />
    </div>
  );
}
