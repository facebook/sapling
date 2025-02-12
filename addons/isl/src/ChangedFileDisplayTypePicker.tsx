/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {isMac} from 'isl-components/OperatingSystem';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom} from 'jotai';
import {useContextMenu} from 'shared/ContextMenu';
import {Row} from './ComponentUtils';
import {t, T} from './i18n';
import {configBackedAtom} from './jotaiUtils';

export type ChangedFilesDisplayType = 'short' | 'fullPaths' | 'tree' | 'fish';

export const defaultChangedFilesDisplayType: ChangedFilesDisplayType = 'short';

export const changedFilesDisplayType = configBackedAtom<ChangedFilesDisplayType>(
  'isl.changedFilesDisplayType',
  defaultChangedFilesDisplayType,
);

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
  const [displayType, setDisplayType] = useAtom(changedFilesDisplayType);

  const actions = entries.map(([type, options]) => ({
    label: (
      <Row>
        <Icon icon={displayType === type ? 'check' : 'blank'} slot="start" />
        <Icon icon={options.icon} slot="start" />
        {options.label}
      </Row>
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
      <Button
        icon
        className="changed-file-display-type-picker"
        data-testid="changed-file-display-type-picker"
        onClick={contextMenu}>
        <Icon icon={ChangedFileDisplayTypeOptions[displayType].icon} />
      </Button>
    </Tooltip>
  );
}
