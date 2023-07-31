/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {debugLogMessageTraffic} from '../ClientToServerAPI';
import {DropdownField, DropdownFields} from '../DropdownFields';
import {Subtle} from '../Subtle';
import {Tooltip} from '../Tooltip';
import {t, T} from '../i18n';
import {getAllRecoilStateJson} from './getAllRecoilStateJson';
import {VSCodeButton, VSCodeCheckbox} from '@vscode/webview-ui-toolkit/react';
import {useState} from 'react';
import {atom, useRecoilCallback, useRecoilState} from 'recoil';

import './DebugToolsMenu.css';

export default function DebugToolsMenu() {
  return (
    <DropdownFields
      title={<T>Internal Debugging Tools</T>}
      icon="pulse"
      data-testid="internal-debug-tools-dropdown"
      className="internal-debug-tools-dropdown">
      <DropdownField title={<T>Internal Recoil State</T>}>
        <InternalState />
      </DropdownField>
      <DropdownField title={<T>Server/Client Messages</T>}>
        <ServerClientMessageLogging />
      </DropdownField>
    </DropdownFields>
  );
}

function InternalState() {
  const [successMessage, setSuccessMessage] = useState<null | string>(null);
  const generate = useRecoilCallback(({snapshot}) => () => {
    const nodes = getAllRecoilStateJson(snapshot);
    // eslint-disable-next-line no-console
    console.log(nodes);
    setSuccessMessage('logged to console!');
  });

  return (
    <div className="internal-debug-tools-recoil-state">
      <Tooltip
        placement="bottom"
        title={t('Capture a snapshot of all recoil atom state, log it to the dev tools console.')}>
        <VSCodeButton onClick={generate} appearance="secondary">
          <T>Take Snapshot</T>
        </VSCodeButton>
        {successMessage && <Subtle>{successMessage}</Subtle>}
      </Tooltip>
    </div>
  );
}

const logMessagesState = atom({
  key: 'logMessagesState',
  default: debugLogMessageTraffic.shoudlLog,
  effects: [
    ({onSet}) => {
      onSet(newValue => {
        debugLogMessageTraffic.shoudlLog = newValue;
        // eslint-disable-next-line no-console
        console.log(`----- ${newValue ? 'Enabled' : 'Disabled'} Logging Messages -----`);
      });
    },
  ],
});

function ServerClientMessageLogging() {
  const [shouldLog, setShouldLog] = useRecoilState(logMessagesState);
  return (
    <div>
      <VSCodeCheckbox
        checked={shouldLog}
        onChange={e => setShouldLog((e.target as HTMLInputElement).checked)}>
        <T>Log messages</T>
      </VSCodeCheckbox>
    </div>
  );
}
