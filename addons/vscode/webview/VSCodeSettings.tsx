/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {ComparisonPanelMode, comparisonPanelMode, setComparisonPanelMode} from './state';
import {Dropdown} from 'isl-components/Dropdown';
import {Tooltip} from 'isl-components/Tooltip';
import {Setting} from 'isl/src/Setting';
import {T, t} from 'isl/src/i18n';
import {useAtomValue} from 'jotai';

export default function VSCodeSettings() {
  const panelMode = useAtomValue(comparisonPanelMode);
  return (
    <Setting title={<T>VS Code Settings</T>}>
      <Tooltip
        title={t(
          'Whether to always open a separate panel to view comparisons, or to open the comparison inside an existing ISL window.',
        )}>
        <div className="dropdown-container setting-inline-dropdown">
          <label>
            <T>Comparison Panel Mode</T>
          </label>
          <Dropdown
            options={Object.values(ComparisonPanelMode).map(name => ({name, value: name}))}
            value={panelMode}
            onChange={event =>
              setComparisonPanelMode(event.currentTarget.value as ComparisonPanelMode)
            }
          />
        </div>
      </Tooltip>
    </Setting>
  );
}
