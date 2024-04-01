/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AbsolutePath} from './types';

import serverAPI from './ClientToServerAPI';
import {DropdownField, DropdownFields} from './DropdownFields';
import {useCommandEvent} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {Tooltip} from './Tooltip';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {Badge} from './components/Badge';
import {Divider} from './components/Divider';
import {T} from './i18n';
import {lazyAtom, writeAtom} from './jotaiUtils';
import {serverCwd} from './repositoryData';
import {repositoryInfo} from './serverAPIState';
import {registerCleanup, registerDisposable} from './utils';
import {VSCodeButton, VSCodeRadio, VSCodeRadioGroup} from '@vscode/webview-ui-toolkit/react';
import {useAtomValue} from 'jotai';
import {Icon} from 'shared/Icon';
import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';
import {minimalDisambiguousPaths} from 'shared/minimalDisambiguousPaths';
import {basename} from 'shared/utils';

export const availableCwds = lazyAtom<Array<AbsolutePath>>(() => {
  // Only request `subscribeToAvailableCwds` when first read the atom.
  registerCleanup(
    availableCwds,
    serverAPI.onConnectOrReconnect(() => {
      serverAPI.postMessage({
        type: 'platform/subscribeToAvailableCwds',
      });
    }),
    import.meta.hot,
  );
  return [];
}, []);

registerDisposable(
  availableCwds,
  serverAPI.onMessageOfType('platform/availableCwds', event =>
    writeAtom(availableCwds, event.options),
  ),
  import.meta.hot,
);

export function CwdSelector() {
  const info = useAtomValue(repositoryInfo);
  const additionalToggles = useCommandEvent('ToggleCwdDropdown');
  if (info?.type !== 'success') {
    return null;
  }
  const repoBasename = basename(info.repoRoot);
  return (
    <Tooltip
      trigger="click"
      component={dismiss => <CwdDetails dismiss={dismiss} />}
      additionalToggles={additionalToggles}
      group="topbar"
      placement="bottom"
      title={
        <T replace={{$shortcut: <Kbd keycode={KeyCode.C} modifiers={[Modifier.ALT]} />}}>
          Repository info & cwd ($shortcut)
        </T>
      }>
      <VSCodeButton appearance="icon" data-testid="cwd-dropdown-button">
        <Icon icon="folder" slot="start" />
        {repoBasename}
      </VSCodeButton>
    </Tooltip>
  );
}

function CwdDetails({dismiss}: {dismiss: () => unknown}) {
  const info = useAtomValue(repositoryInfo);
  const repoRoot = info?.type === 'success' ? info.repoRoot : null;
  const provider = useAtomValue(codeReviewProvider);
  const cwd = useAtomValue(serverCwd);
  return (
    <DropdownFields title={<T>Repository info</T>} icon="folder" data-testid="cwd-details-dropdown">
      <CwdSelections dismiss={dismiss} divider />
      <DropdownField title={<T>Active repository</T>}>
        <code>{cwd}</code>
      </DropdownField>
      <DropdownField title={<T>Repository Root</T>}>
        <code>{repoRoot}</code>
      </DropdownField>
      {provider != null ? (
        <DropdownField title={<T>Code Review Provider</T>}>
          <span>
            <Badge>{provider?.name}</Badge> <provider.RepoInfo />
          </span>
        </DropdownField>
      ) : null}
    </DropdownFields>
  );
}

export function CwdSelections({dismiss, divider}: {dismiss: () => unknown; divider?: boolean}) {
  const currentCwd = useAtomValue(serverCwd);
  const cwdOptions = useAtomValue(availableCwds);
  if (cwdOptions.length < 2) {
    return null;
  }

  const paths = minimalDisambiguousPaths(cwdOptions);

  return (
    <DropdownField title={<T>Change active repository</T>}>
      <VSCodeRadioGroup
        orientation="vertical"
        value={currentCwd}
        onChange={e => {
          const newCwd = (e.target as HTMLOptionElement).value as string;
          if (newCwd === currentCwd) {
            // nothing to change
            return;
          }
          serverAPI.postMessage({
            type: 'changeCwd',
            cwd: newCwd,
          });
          serverAPI.cwdChanged();
          dismiss();
        }}>
        {paths.map((shortCwd, index) => {
          const fullCwd = cwdOptions[index];
          return (
            <VSCodeRadio
              key={shortCwd}
              value={fullCwd}
              checked={fullCwd === currentCwd}
              tabIndex={0}>
              <Tooltip key={shortCwd} title={fullCwd} placement="right">
                {shortCwd}
              </Tooltip>
            </VSCodeRadio>
          );
        })}
      </VSCodeRadioGroup>
      {divider && <Divider />}
    </DropdownField>
  );
}
