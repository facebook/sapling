/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AbsolutePath, CwdInfo} from './types';

import serverAPI from './ClientToServerAPI';
import {Row} from './ComponentUtils';
import {DropdownField, DropdownFields} from './DropdownFields';
import {useCommandEvent} from './ISLShortcuts';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {T, t} from './i18n';
import {writeAtom} from './jotaiUtils';
import foundPlatform from './platform';
import {serverCwd} from './repositoryData';
import {repositoryInfo} from './serverAPIState';
import {registerCleanup, registerDisposable} from './utils';
import {Badge} from 'isl-components/Badge';
import {Button} from 'isl-components/Button';
import {ButtonDropdown} from 'isl-components/ButtonDropdown';
import {Divider} from 'isl-components/Divider';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {KeyCode, Modifier} from 'isl-components/KeyboardShortcuts';
import {RadioGroup} from 'isl-components/Radio';
import {Subtle} from 'isl-components/Subtle';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtomValue} from 'jotai';
import {Suspense} from 'react';
import {basename} from 'shared/utils';

/**
 * Give the relative path to `path` from `root`
 * For example, relativePath('/home/user', '/home') -> 'user'
 */
export function relativePath(root: AbsolutePath, path: AbsolutePath) {
  if (root == null || path === '') {
    return '';
  }
  return path.replace(root, '');
}

/**
 * Trim a suffix if it exists
 * maybeTrim('abc/', '/') -> 'abc'
 * maybeTrim('abc', '/') -> 'abc'
 */
function maybeTrim(s: string, c: string): string {
  return s.endsWith(c) ? s.slice(0, -c.length) : s;
}

function getRepoLabel(repoRoot: AbsolutePath, cwd: string) {
  const sep = guessPathSep(cwd);
  const repoBasename = maybeTrim(basename(repoRoot, sep), sep);
  const repoRelativeCwd = relativePath(repoRoot, cwd);
  if (repoRelativeCwd === '') {
    return repoBasename;
  }
  return repoBasename + repoRelativeCwd;
}

export const availableCwds = atom<Array<CwdInfo>>([]);
registerCleanup(
  availableCwds,
  serverAPI.onConnectOrReconnect(() => {
    serverAPI.postMessage({
      type: 'platform/subscribeToAvailableCwds',
    });
  }),
  import.meta.hot,
);

registerDisposable(
  availableCwds,
  serverAPI.onMessageOfType('platform/availableCwds', event =>
    writeAtom(availableCwds, event.options),
  ),
  import.meta.hot,
);

export function CwdSelector() {
  const info = useAtomValue(repositoryInfo);
  const currentCwd = useAtomValue(serverCwd);
  const additionalToggles = useCommandEvent('ToggleCwdDropdown');
  const allOptions = useCwdOptions();
  const options = allOptions.filter(opt => opt.valid);
  if (info?.type !== 'success') {
    return null;
  }
  const repoLabel = getRepoLabel(info.repoRoot, currentCwd);
  return (
    <Tooltip
      trigger="click"
      component={dismiss => <CwdDetails dismiss={dismiss} />}
      additionalToggles={additionalToggles.asEventTarget()}
      group="topbar"
      placement="bottom"
      title={
        <T replace={{$shortcut: <Kbd keycode={KeyCode.C} modifiers={[Modifier.ALT]} />}}>
          Repository info & cwd ($shortcut)
        </T>
      }>
      {options.length < 2 ? (
        <Button icon data-testid="cwd-dropdown-button">
          <Icon icon="folder" />
          {repoLabel}
        </Button>
      ) : (
        // use a ButtonDropdown as a shortcut to quickly change cwd
        <ButtonDropdown
          data-testid="cwd-dropdown-button"
          kind="icon"
          options={options}
          selected={
            options.find(opt => opt.id === currentCwd) ?? {
              id: currentCwd,
              label: repoLabel,
              valid: true,
            }
          }
          icon={<Icon icon="folder" />}
          onClick={
            () => null // fall through to the Tooltip
          }
          onChangeSelected={value => {
            if (value.id !== currentCwd) {
              changeCwd(value.id);
            }
          }}></ButtonDropdown>
      )}
    </Tooltip>
  );
}

function CwdDetails({dismiss}: {dismiss: () => unknown}) {
  const info = useAtomValue(repositoryInfo);
  const repoRoot = info?.type === 'success' ? info.repoRoot : null;
  const provider = useAtomValue(codeReviewProvider);
  const cwd = useAtomValue(serverCwd);
  const AddMoreCwdsHint = foundPlatform.AddMoreCwdsHint;
  return (
    <DropdownFields title={<T>Repository info</T>} icon="folder" data-testid="cwd-details-dropdown">
      <CwdSelections dismiss={dismiss} divider />
      {AddMoreCwdsHint && (
        <Suspense>
          <AddMoreCwdsHint />
        </Suspense>
      )}
      <DropdownField title={<T>Active working directory</T>}>
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

function changeCwd(newCwd: string) {
  serverAPI.postMessage({
    type: 'changeCwd',
    cwd: newCwd,
  });
  serverAPI.cwdChanged();
}

function useCwdOptions() {
  const cwdOptions = useAtomValue(availableCwds);

  return cwdOptions.map((cwd, index) => ({
    id: cwdOptions[index].cwd,
    label: cwd.repoRelativeCwdLabel ?? cwd.cwd,
    valid: cwd.repoRoot != null,
  }));
}

function guessPathSep(path: string): '/' | '\\' {
  if (path.includes('\\')) {
    return '\\';
  } else {
    return '/';
  }
}

export function CwdSelections({dismiss, divider}: {dismiss: () => unknown; divider?: boolean}) {
  const currentCwd = useAtomValue(serverCwd);
  const options = useCwdOptions();
  if (options.length < 2) {
    return null;
  }

  return (
    <DropdownField title={<T>Change active working directory</T>}>
      <RadioGroup
        choices={options.map(({id, label, valid}) => ({
          title: valid ? (
            label
          ) : (
            <Row key={id}>
              {label}{' '}
              <Subtle>
                <T>Not a valid repository</T>
              </Subtle>
            </Row>
          ),
          value: id,
          tooltip: valid
            ? id
            : t('Path $path does not appear to be a valid Sapling repository', {
                replace: {$path: id},
              }),
          disabled: !valid,
        }))}
        current={currentCwd}
        onChange={newCwd => {
          if (newCwd === currentCwd) {
            // nothing to change
            return;
          }
          changeCwd(newCwd);
          dismiss();
        }}
      />
      {divider && <Divider />}
    </DropdownField>
  );
}
