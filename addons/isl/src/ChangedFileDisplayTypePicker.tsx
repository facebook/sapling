/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {FlexRow} from './ComponentUtils';
import {Tooltip} from './Tooltip';
import {t, T} from './i18n';
import {persistAtomToConfigEffect} from './persistAtomToConfigEffect';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {atom, useRecoilState} from 'recoil';
import {useContextMenu} from 'shared/ContextMenu';
import {Icon} from 'shared/Icon';
import {isMac} from 'shared/OperatingSystem';

export type ChangedFilesDisplayType = 'short' | 'fullPaths' | 'tree' | 'fish';

export const changedFilesDisplayType = atom<ChangedFilesDisplayType>({
  key: 'changedFilesDisplayType',
  default: 'short',
  effects: [
    persistAtomToConfigEffect('isl.changedFilesDisplayType', 'short' as ChangedFilesDisplayType),
  ],
});

type ChangedFileDisplayTypeOption = {icon: string; label: JSX.Element};
const ChangedFileDisplayTypeOptions: Record<ChangedFilesDisplayType, ChangedFileDisplayTypeOption> =
  {
    short: {icon: 'list-selection', label: <T>Short file names</T>},
    fullPaths: {icon: 'menu', label: <T>Full file paths</T>},
    tree: {icon: 'list-tree', label: <T>Tree</T>},
    fish: {icon: 'whole-word', label: <T>One-letter directories</T>},
  };
const entries = Object.entries(ChangedFileDisplayTypeOptions) as Array<
  [ChangedFilesDisplayType, ChangedFileDisplayTypeOption]
>;

export function ChangedFileDisplayTypePicker() {
  const [displayType, setDisplayType] = useRecoilState(changedFilesDisplayType);

  const actions = entries.map(([type, options]) => ({
    label: (
      <FlexRow>
        <Icon icon={displayType === type ? 'check' : 'blank'} slot="start" />
        <Icon icon={options.icon} slot="start" />
        {options.label}
      </FlexRow>
    ),
    onClick: () => setDisplayType(type),
  }));
  const contextMenu = useContextMenu(() => actions);

  return (
    <Tooltip
      title={t(
        isMac
          ? 'Change how file paths are displayed.\n\nTip: Hold the alt key to quickly see full file paths.'
          : 'Change how file paths are displayed.\n\nTip: Hold the ctrl key to quickly see full file paths.',
      )}>
      <VSCodeButton
        appearance="icon"
        className="changed-file-display-type-picker"
        data-testid="changed-file-display-type-picker"
        onClick={contextMenu}>
        <Icon icon={ChangedFileDisplayTypeOptions[displayType].icon} />
      </VSCodeButton>
    </Tooltip>
  );
}
