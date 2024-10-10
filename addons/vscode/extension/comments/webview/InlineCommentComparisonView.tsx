/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Context} from 'isl/src/ComparisonView/SplitDiffView/types';
import type {ParsedDiff} from 'shared/patch/parse';

import stylex from '@stylexjs/stylex';
import {Icon} from 'isl-components/Icon';
import {SplitDiffTable} from 'isl/src/ComparisonView/SplitDiffView/SplitDiffHunk';
import {Modal} from 'isl/src/Modal';
import {Suspense} from 'react';

import 'isl/src/ComparisonView/ComparisonView.css';

const style = stylex.create({
  smallBtn: {
    padding: '0 5px',
  },
  alignTop: {alignItems: 'flex-start'},
});

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
    <Modal className="comparison-view-modal" height="" width="">
      <Suspense fallback={<Icon icon="loading" />}>
        <div data-testid="comparison-view" className="comparison-view">
          {/* <ComparisonViewHeader
            comparison={comparison}
            collapsedFiles={collapsedFiles}
            setCollapsedFile={setCollapsedFile}
            dismiss={dismiss}
          /> */}
          <div className="comparison-view-details">
            <div className="comparison-view-file" key={path}>
              <div className="split-diff-view">
                <div className="split-diff-view">
                  <SplitDiffTable ctx={ctx} path={path} patch={suggestion} />
                </div>
              </div>
            </div>
          </div>
        </div>
      </Suspense>
    </Modal>
  );
}
