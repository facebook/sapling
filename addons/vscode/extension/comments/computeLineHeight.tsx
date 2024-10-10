/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as vscode from 'vscode';

const MAC_LINE_HEIGHT_RATIO = 1.5;
const OTHER_LINE_HEIGHT_RATIO = 1.35;
const MINIMUM_LINE_HEIGHT = 8;

/**
 * https://github.com/microsoft/vscode/blob/main/src/vs/editor/common/config/fontInfo.ts
 */
export default function computeLineHeight(isMac: boolean): number {
  const editorConfig = vscode.workspace.getConfiguration('editor');
  const lineHeightConfig = editorConfig.get<number>('lineHeight') ?? 0;
  const fontSizeConfig = editorConfig.get<number>('fontSize') ?? 12;
  const lineHeightRatio = isMac ? MAC_LINE_HEIGHT_RATIO : OTHER_LINE_HEIGHT_RATIO;
  let lineHeight = lineHeightConfig;

  if (lineHeight === 0) {
    lineHeight = lineHeightRatio * fontSizeConfig;
  } else if (lineHeight < MINIMUM_LINE_HEIGHT) {
    lineHeight = lineHeight * fontSizeConfig;
  }

  return Math.max(MINIMUM_LINE_HEIGHT, Math.round(lineHeight));
}
