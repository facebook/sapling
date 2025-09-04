/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ReactNode} from 'react';
import type {
  AbsolutePath,
  CwdInfo,
  CwdRelativePath,
  RepoRelativePath,
  ValidatedRepoInfo,
} from './types';

import * as stylex from '@stylexjs/stylex';
import {Badge} from 'isl-components/Badge';
import {Button, buttonStyles} from 'isl-components/Button';
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
import {colors} from '../../components/theme/tokens.stylex';
import serverAPI from './ClientToServerAPI';
import {Row} from './ComponentUtils';
import {DropdownField, DropdownFields} from './DropdownFields';
import {useCommandEvent} from './ISLShortcuts';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {T, t} from './i18n';
import {writeAtom} from './jotaiUtils';
import platform from './platform';
import {serverCwd} from './repositoryData';
import {repositoryInfo, submodulesByRoot} from './serverAPIState';
import {registerCleanup, registerDisposable} from './utils';

/**
 * Give the relative path to `path` from `root`
 * For example, relativePath('/home/user', '/home') -> 'user'
 */
export function relativePath(root: AbsolutePath, path: AbsolutePath) {
  if (root == null || path === '') {
    return '';
  }
  const sep = guessPathSep(path);
  return maybeTrimPrefix(path.replace(root, ''), sep);
}

/**
 * Simple version of path.join()
 * Expect an absolute root path and a relative path
 * e.g.
 * joinPaths('/home', 'user') -> '/home/user'
 * joinPaths('/home/', 'user/.config') -> '/home/user/.config'
 */
export function joinPaths(root: AbsolutePath, path: CwdRelativePath, sep = '/'): AbsolutePath {
  return root.endsWith(sep) ? root + path : root + sep + path;
}

/**
 * Trim a suffix if it exists
 * maybeTrimSuffix('abc/', '/') -> 'abc'
 * maybeTrimSuffix('abc', '/') -> 'abc'
 */
function maybeTrimSuffix(s: string, c: string): string {
  return s.endsWith(c) ? s.slice(0, -c.length) : s;
}

function maybeTrimPrefix(s: string, c: string): string {
  return s.startsWith(c) ? s.slice(c.length) : s;
}

function getMainSelectorLabel(
  directRepoRoot: AbsolutePath,
  nestedRepoRoots: AbsolutePath[] | undefined,
  cwd: string,
) {
  const sep = guessPathSep(cwd);

  // If there are multiple nested repo roots,
  // show the first one as there will be following selectors for the rest
  if (nestedRepoRoots && nestedRepoRoots.length > 1) {
    return maybeTrimSuffix(basename(nestedRepoRoots[0], sep), sep);
  }

  // Otherwise, build the label with the direct and only repo root
  const repoBasename = maybeTrimSuffix(basename(directRepoRoot, sep), sep);
  const repoRelativeCwd = relativePath(directRepoRoot, cwd);
  if (repoRelativeCwd === '') {
    return repoBasename;
  }
  return joinPaths(repoBasename, repoRelativeCwd, sep);
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

const styles = stylex.create({
  container: {
    display: 'flex',
    gap: 0,
  },
  hideRightBorder: {
    borderRight: 0,
    marginRight: 0,
    borderTopRightRadius: 0,
    borderBottomRightRadius: 0,
  },
  hideLeftBorder: {
    borderLeft: 0,
    marginLeft: 0,
    borderTopLeftRadius: 0,
    borderBottomLeftRadius: 0,
  },
  submoduleSelect: {
    appearance: 'none', // remove default styling of <select/>
    width: 'auto',
    maxWidth: '96px',
    textOverflow: 'ellipsis',
    boxShadow: 'none',
    outline: 'none',
  },
  submoduleSeparator: {
    // Override background to disable hover effect
    background: {
      default: colors.subtleHoverDarken,
    },
  },
});

export function CwdSelector() {
  const info = useAtomValue(repositoryInfo);
  const currentCwd = useAtomValue(serverCwd);
  const allSubmoduleOptions = useSubmoduleOptions(info);
  if (info == null) {
    return null;
  }
  // The most direct root of the cwd
  const repoRoot = info.repoRoot;
  // The list of repo roots down to the cwd, in order from furthest to closest
  const repoRoots = info.repoRoots;

  const mainLabel = getMainSelectorLabel(repoRoot, repoRoots, currentCwd);

  return (
    <div {...stylex.props(styles.container)}>
      <MainCwdSelector
        currentCwd={currentCwd}
        label={mainLabel}
        hideRightBorder={
          (repoRoots && repoRoots.length > 1) ||
          (allSubmoduleOptions?.get(repoRoot)?.length ?? 0) > 0
        }
      />
      <SubmoduleSelectorGroup repoRoots={repoRoots} submoduleOptions={allSubmoduleOptions} />
    </div>
  );
}

/**
 * The leftmost tooltip that can show cwd and repo details.
 */
function MainCwdSelector({
  currentCwd,
  label,
  hideRightBorder,
}: {
  currentCwd: AbsolutePath;
  label: string;
  hideRightBorder: boolean;
}) {
  const allCwdOptions = useCwdOptions();
  const cwdOptions = allCwdOptions.filter(opt => opt.valid);
  const additionalToggles = useCommandEvent('ToggleCwdDropdown');

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
      {hideRightBorder || cwdOptions.length < 2 ? (
        <Button
          icon
          data-testid="cwd-dropdown-button"
          {...stylex.props(hideRightBorder && styles.hideRightBorder)}>
          <Icon icon="folder" />
          {label}
        </Button>
      ) : (
        // use a ButtonDropdown as a shortcut to quickly change cwd
        <ButtonDropdown
          data-testid="cwd-dropdown-button"
          kind="icon"
          options={cwdOptions}
          selected={
            cwdOptions.find(opt => opt.id === currentCwd) ?? {
              id: currentCwd,
              label,
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

function SubmoduleSelectorGroup({
  repoRoots,
  submoduleOptions,
}: {
  repoRoots: AbsolutePath[] | undefined;
  submoduleOptions: Map<
    AbsolutePath,
    {id: RepoRelativePath; label: string; valid: boolean}[] | undefined
  >;
}) {
  const currentCwd = useAtomValue(serverCwd);
  if (repoRoots == null) {
    return null;
  }

  const numRoots = repoRoots.length;
  const directRepoRoot = repoRoots[numRoots - 1];
  const submodulesToBeSelected = submoduleOptions.get(directRepoRoot);

  const out = [];
  for (let i = 1; i < numRoots; i++) {
    const currRoot = repoRoots[i];
    const prevRoot = repoRoots[i - 1];
    const submodules = submoduleOptions.get(prevRoot);
    if (submodules != null && submodules.length > 0) {
      out.push(
        <SubmoduleSelector
          options={submodules}
          selected={submodules?.find(opt => opt.id === relativePath(prevRoot, currRoot))}
          onChangeSelected={value => {
            if (value.id !== relativePath(prevRoot, currRoot)) {
              changeCwd(joinPaths(prevRoot, value.id));
            }
          }}
          hideRightBorder={i < numRoots - 1 || submodulesToBeSelected != undefined}
          key={prevRoot}
        />,
      );
    }
  }

  if (submodulesToBeSelected != undefined) {
    out.push(
      <SubmoduleSelector
        options={submodulesToBeSelected}
        onChangeSelected={value => {
          if (value.id !== relativePath(directRepoRoot, currentCwd)) {
            changeCwd(joinPaths(directRepoRoot, value.id));
          }
        }}
        hideRightBorder={false}
        key={repoRoots[numRoots - 1]}
      />,
    );
  }

  return out;
}

function CwdDetails({dismiss}: {dismiss: () => unknown}) {
  const info = useAtomValue(repositoryInfo);
  const repoRoot = info?.repoRoot ?? null;
  const provider = useAtomValue(codeReviewProvider);
  const cwd = useAtomValue(serverCwd);
  const AddMoreCwdsHint = platform.AddMoreCwdsHint;
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

function useSubmoduleOptions(
  info: ValidatedRepoInfo | undefined,
): Map<AbsolutePath, {id: RepoRelativePath; label: string; valid: boolean}[] | undefined> {
  const repoRoots = info?.repoRoots;
  const submodulesMap = useAtomValue(submodulesByRoot);

  return new Map(
    repoRoots?.map(root => {
      const fetchedSubmodules = submodulesMap.get(root);
      return [
        root,
        fetchedSubmodules?.value?.map(m => ({
          id: m.path,
          label: m.name,
          valid: m.active,
        })),
      ];
    }),
  );
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

/**
 * Dropdown selector for submodules in a breadcrumb style.
 */
function SubmoduleSelector<T extends {label: ReactNode; id: string}>({
  options,
  selected,
  onChangeSelected,
  hideRightBorder = true,
}: {
  options: ReadonlyArray<T>;
  selected?: T;
  onChangeSelected: (newSelected: T) => unknown;
  hideRightBorder?: boolean;
}) {
  const selectedValue = options.find(opt => opt.id === selected?.id)?.id;

  return (
    <Tooltip
      trigger="hover"
      placement="bottom"
      component={() => <SubmoduleHint path={selectedValue} />}>
      <Icon
        icon="chevron-right"
        {...stylex.props(
          buttonStyles.icon,
          styles.submoduleSeparator,
          styles.hideLeftBorder,
          styles.hideRightBorder,
        )}
      />
      <select
        {...stylex.props(
          buttonStyles.button,
          buttonStyles.icon,
          styles.submoduleSelect,
          styles.hideLeftBorder,
          hideRightBorder && styles.hideRightBorder,
        )}
        value={selectedValue ?? ''}
        onChange={event => {
          const matching = options.find(opt => opt.id === (event.target.value as T['id']));
          if (matching != null) {
            onChangeSelected(matching);
          }
        }}>
        <option value="" disabled hidden>
          submodules ...
        </option>
        {options.map(option => (
          <option key={option.id} value={option.id}>
            {option.label}
          </option>
        ))}
      </select>
    </Tooltip>
  );
}

function SubmoduleHint({path}: {path: string | undefined}) {
  return <T>{path ? `Submodule at: ${path}` : 'Select a submodule'}</T>;
}
