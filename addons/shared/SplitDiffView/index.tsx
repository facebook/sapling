/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './SplitDiffHunk.css';

import type {ParsedDiff} from '../patch/parse';
import type {SplitDiffTableProps} from './SplitDiffHunk';
import type {Context} from './types';

import {DiffType} from '../patch/parse';
import {FileHeader} from './SplitDiffFileHeader';
import {SplitDiffTable} from './SplitDiffHunk';
import {Box} from '@primer/react';

export function SplitDiffView<Id>({
  ctx,
  path,
  patch,
}: {
  ctx: Context<Id>;
  path: string;
  patch: ParsedDiff;
}) {
  const fileName = patch.newFileName ?? patch.oldFileName ?? '/dev/null';

  // Type hack to get a templatized version of a React.memo-ized component
  const TypedSplitDiffTable = SplitDiffTable as unknown as React.FC<SplitDiffTableProps<Id>>;

  const t = ctx.translate ?? (s => s);

  const preamble = [];
  if (patch.type === DiffType.Added) {
    preamble.push(<FileStatusBanner key="added">{t('This file was added')}</FileStatusBanner>);
  }
  if (patch.type === DiffType.Removed) {
    preamble.push(<FileStatusBanner key="deleted">{t('This file was removed')}</FileStatusBanner>);
  }
  if (patch.type === DiffType.Renamed) {
    preamble.push(
      <FileStatusBanner key="renamed">
        {t('This file was renamed from')} {patch.oldFileName ?? ''}
      </FileStatusBanner>,
    );
  }
  if (patch.type === DiffType.Copied) {
    preamble.push(
      <FileStatusBanner key="copied">
        {t('This file was copied from')} {patch.oldFileName ?? ''}
      </FileStatusBanner>,
    );
  }

  return (
    <Box
      className="split-diff-view"
      borderWidth="1px"
      borderStyle="solid"
      borderColor="border.default"
      borderRadius={2}>
      <FileHeader ctx={ctx} path={fileName} diffType={patch.type} />
      <TypedSplitDiffTable ctx={ctx} path={path} patch={patch} preamble={preamble} />
    </Box>
  );
}

function FileStatusBanner({children}: {children: React.ReactNode}): React.ReactElement {
  return (
    <Box as="tr" bg="attention.subtle" color="fg.muted" height={12}>
      <Box as="td" colSpan={4} className="separator">
        <Box padding={1}>{children}</Box>
      </Box>
    </Box>
  );
}
