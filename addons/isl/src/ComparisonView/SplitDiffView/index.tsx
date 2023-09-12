/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './SplitDiffHunk.css';

import type {SplitDiffTableProps} from './SplitDiffHunk';
import type {Context} from './types';
import type {ParsedDiff} from 'shared/patch/parse';

import {FileHeader} from './SplitDiffFileHeader';
import {SplitDiffTable} from './SplitDiffHunk';
import {DiffType} from 'shared/patch/parse';

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
  const collapsed = ctx.collapsed;

  // Type hack to get a templatized version of a React.memo-ized component
  const TypedSplitDiffTable = SplitDiffTable as unknown as React.FC<SplitDiffTableProps<Id>>;

  const t = ctx.translate ?? (s => s);

  const preamble = [];
  if (patch.type === DiffType.Added) {
    preamble.push(
      <FileStatusBanner key="added" color="added">
        {t('This file was added')}
      </FileStatusBanner>,
    );
  }
  if (patch.type === DiffType.Removed) {
    preamble.push(
      <FileStatusBanner key="deleted" color="removed">
        {t('This file was removed')}
      </FileStatusBanner>,
    );
  }
  if (patch.type === DiffType.Renamed) {
    preamble.push(
      <FileStatusBanner key="renamed" color="modified">
        {t('This file was renamed from')} {patch.oldFileName ?? ''}
      </FileStatusBanner>,
    );
  }
  if (patch.type === DiffType.Copied) {
    preamble.push(
      <FileStatusBanner key="copied" color="added">
        {t('This file was copied from')} {patch.oldFileName ?? ''}
      </FileStatusBanner>,
    );
  }

  return (
    <div className="split-diff-view">
      <FileHeader
        path={fileName}
        diffType={patch.type}
        open={!collapsed}
        onChangeOpen={open => ctx.setCollapsed(!open)}
      />
      {!collapsed && (
        <TypedSplitDiffTable ctx={ctx} path={path} patch={patch} preamble={preamble} />
      )}
    </div>
  );
}

function FileStatusBanner({
  children,
  color,
}: {
  children: React.ReactNode;
  color: 'added' | 'removed' | 'modified';
}): React.ReactElement {
  return (
    <tr className={`split-diff-view-file-status-banner split-diff-view-banner-${color}`}>
      <td colSpan={4} className="separator">
        <div className="split-diff-view-">{children}</div>
      </td>
    </tr>
  );
}
