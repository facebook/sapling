/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import './SplitDiffHunk.css';

import {guessIsSubmodule} from 'shared/patch/parse';
import {type ParsedDiff} from 'shared/patch/types';
import type {Context} from './types';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Tooltip} from 'isl-components/Tooltip';
import {useState} from 'react';
import {generatedStatusDescription} from '../../GeneratedFile';
import {T, t} from '../../i18n';
import platform from '../../platform';
import {GeneratedStatus} from '../../types';
import {FileHeader, diffTypeToIconType} from './SplitDiffFileHeader';
import {SplitDiffTable} from './SplitDiffHunk';

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

  const isGenerated = generatedStatus != null && generatedStatus === GeneratedStatus.Generated;
  // whether the file content is collapsed due to being generated
  const [isContentCollapsed, setIsContentCollapsed] = useState(isGenerated);

  const preamble = [];
  if (generatedStatus != null && generatedStatus !== GeneratedStatus.Manual) {
    preamble.push(
      <FileStatusBanner key="generated" color={'modified'}>
        <div>{generatedStatusDescription(generatedStatus)}</div>
        {isContentCollapsed ? (
          <Button icon onClick={() => setIsContentCollapsed(false)}>
            <T>Show anyway</T>
          </Button>
        ) : null}
      </FileStatusBanner>,
    );
  }

  const isSubmodule = guessIsSubmodule(patch);
  const fileActions = (
    <>
      {platform.openDiff == null ? null : (
        <Tooltip title={t('Open diff view for file')} placement={'bottom'}>
          <Button
            icon
            className="split-diff-view-file-header-open-diff-button"
            onClick={() => {
              platform.openDiff?.(path, ctx.id.comparison);
            }}>
            <Icon icon="git-pull-request-go-to-changes" />
          </Button>
        </Tooltip>
      )}
      {!isSubmodule && (
        <Tooltip title={t('Open file')} placement={'bottom'}>
          <Button
            icon
            className="split-diff-view-file-header-open-button"
            onClick={() => {
              platform.openFile(path);
            }}>
            <Icon icon="go-to-file" />
          </Button>
        </Tooltip>
      )}
    </>
  );

  const copyFrom = patch.oldFileName === fileName ? undefined : patch.oldFileName;
  const iconType = diffTypeToIconType(patch.type);
  return (
    <div className="split-diff-view">
      <FileHeader
        path={fileName}
        copyFrom={copyFrom}
        iconType={iconType}
        open={!collapsed}
        onChangeOpen={open => ctx.setCollapsed(!open)}
        fileActions={fileActions}
      />
      {!collapsed && preamble && <div className="split-diff-view-file-preamble">{preamble}</div>}
      {!collapsed && !isContentCollapsed && <SplitDiffTable ctx={ctx} path={path} patch={patch} />}
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
