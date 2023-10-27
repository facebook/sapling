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

import {generatedStatusDescription} from '../../GeneratedFile';
import {Tooltip} from '../../Tooltip';
import {T} from '../../i18n';
import platform from '../../platform';
import {GeneratedStatus} from '../../types';
import {FileHeader} from './SplitDiffFileHeader';
import {SplitDiffTable} from './SplitDiffHunk';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useState} from 'react';
import {Icon} from 'shared/Icon';
import {DiffType} from 'shared/patch/parse';

export function SplitDiffView({
  ctx,
  path,
  patch,
  generatedStatus,
}: {
  ctx: Context;
  path: string;
  patch: ParsedDiff;
  generatedStatus?: GeneratedStatus;
}) {
  const fileName = patch.newFileName ?? patch.oldFileName ?? '/dev/null';
  // whether the file is manually or automatically collapsed by the chevron
  const collapsed = ctx.collapsed;

  // Type hack to get a templatized version of a React.memo-ized component
  const TypedSplitDiffTable = SplitDiffTable as unknown as React.FC<SplitDiffTableProps>;

  const t = ctx.translate ?? (s => s);

  const isGenerated = generatedStatus != null && generatedStatus === GeneratedStatus.Generated;
  // whether the file content is collapsed due to being generated
  const [isContentCollapsed, setIsContentCollapsed] = useState(isGenerated);

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
  if (generatedStatus != null && generatedStatus !== GeneratedStatus.Manual) {
    preamble.push(
      <FileStatusBanner key="generated" color={'modified'}>
        <div>{generatedStatusDescription(generatedStatus)}</div>
        {isContentCollapsed ? (
          <VSCodeButton appearance="icon" onClick={() => setIsContentCollapsed(false)}>
            <T>Show anyway</T>
          </VSCodeButton>
        ) : null}
      </FileStatusBanner>,
    );
  }

  const fileActions = (
    <>
      {platform.openDiff == null ? null : (
        <Tooltip title={t('Open diff view for file')} placement={'bottom'}>
          <VSCodeButton
            appearance="icon"
            className="split-diff-view-file-header-open-diff-button"
            onClick={() => {
              platform.openDiff?.(path, ctx.id.comparison);
            }}>
            <Icon icon="git-pull-request-go-to-changes" />
          </VSCodeButton>
        </Tooltip>
      )}
      <Tooltip title={t('Open file')} placement={'bottom'}>
        <VSCodeButton
          appearance="icon"
          className="split-diff-view-file-header-open-button"
          onClick={() => {
            platform.openFile(path);
          }}>
          <Icon icon="go-to-file" />
        </VSCodeButton>
      </Tooltip>
    </>
  );

  return (
    <div className="split-diff-view">
      <FileHeader
        path={fileName}
        diffType={patch.type}
        open={!collapsed}
        onChangeOpen={open => ctx.setCollapsed(!open)}
        fileActions={fileActions}
      />
      {!collapsed && preamble && <div className="split-diff-view-file-preamble">{preamble}</div>}
      {!collapsed && !isContentCollapsed && (
        <TypedSplitDiffTable ctx={ctx} path={path} patch={patch} />
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
    <div className={`split-diff-view-file-status-banner split-diff-view-banner-${color}`}>
      {children}
    </div>
  );
}
