/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from './ClientToServerAPI';
import {Icon} from './Icon';
import {PullButton} from './PullButton';
import {SettingsGearButton} from './SettingsTooltip';
import {DOCUMENTATION_DELAY, Tooltip} from './Tooltip';
import {t} from './i18n';
import {haveCommitsLoadedYet, useClearAllOptimisticState} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';

import './TopBar.css';

export function TopBar() {
  const loaded = useRecoilValue(haveCommitsLoadedYet);
  if (!loaded) {
    return null;
  }
  return (
    <div className="top-bar">
      <PullButton />
      <span className="button-group">
        <SettingsGearButton />
        <RefreshButton />
      </span>
    </div>
  );
}

function RefreshButton() {
  const clearOptimisticState = useClearAllOptimisticState();
  return (
    <Tooltip
      delayMs={DOCUMENTATION_DELAY}
      placement="bottom"
      title={t('Re-fetch latest commits and uncommitted changes.')}>
      <VSCodeButton
        appearance="secondary"
        onClick={() => {
          clearOptimisticState();
          serverAPI.postMessage({type: 'refresh'});
        }}
        data-testid="refresh-button">
        <Icon icon="refresh" />
      </VSCodeButton>
    </Tooltip>
  );
}
