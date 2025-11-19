/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {
  AbsolutePath,
  CwdInfo,
  CwdRelativePath,
  RepoRelativePath,
  Submodule,
  SubmodulesByRoot,
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
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtomValue} from 'jotai';
import {Suspense, useState} from 'react';
import {basename} from 'shared/utils';
import {colors, spacing} from '../../components/theme/tokens.stylex';
import serverAPI from './ClientToServerAPI';
import {Column, Row, ScrollY} from './ComponentUtils';
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
  submoduleDropdownContainer: {
    alignItems: 'flex-start',
    gap: spacing.pad,
  },
  submoduleList: {
    width: '100%',
    overflow: 'hidden',
  },
  submoduleOption: {
    padding: 'var(--halfpad)',
    borderRadius: 'var(--halfpad)',
    cursor: 'pointer',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    boxSizing: 'border-box',
    backgroundColor: {
      ':hover': 'var(--hover-darken)',
      ':focus': 'var(--hover-darken)',
    },
    width: '100%',
  },
});

export function CwdSelector() {
  const info = useAtomValue(repositoryInfo);
  const currentCwd = useAtomValue(serverCwd);
  const submodulesMap = useAtomValue(submodulesByRoot);

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
          (submodulesMap?.get(repoRoot)?.value?.length ?? 0) > 0
        }
      />
      <SubmoduleSelectorGroup repoRoots={repoRoots} submoduleOptions={submodulesMap} />
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
  submoduleOptions: SubmodulesByRoot;
}) {
  const currentCwd = useAtomValue(serverCwd);
  if (repoRoots == null) {
    return null;
  }
  const numRoots = repoRoots.length;
  const directRepoRoot = repoRoots[numRoots - 1];
  if (currentCwd !== directRepoRoot) {
    // If the actual cwd is deeper than the supeproject root,
    // submodule selectors don't make sense
    return null;
  }
  const submodulesToBeSelected = submoduleOptions.get(directRepoRoot)?.value;

  const out = [];

  for (let i = 1; i < numRoots; i++) {
    const currRoot = repoRoots[i];
    const prevRoot = repoRoots[i - 1];
    const submodules = submoduleOptions.get(prevRoot)?.value;
    if (submodules != null && submodules.length > 0) {
      out.push(
        <SubmoduleSelector
          submodules={submodules}
          selected={submodules?.find(opt => opt.path === relativePath(prevRoot, currRoot))}
          onChangeSelected={value => {
            if (value.path !== relativePath(prevRoot, currRoot)) {
              changeCwd(joinPaths(prevRoot, value.path));
            }
          }}
          hideRightBorder={i < numRoots - 1 || submodulesToBeSelected != undefined}
          root={prevRoot}
          key={prevRoot}
        />,
      );
    }
  }

  if (submodulesToBeSelected != undefined) {
    out.push(
      <SubmoduleSelector
        submodules={submodulesToBeSelected}
        onChangeSelected={value => {
          if (value.path !== relativePath(directRepoRoot, currentCwd)) {
            changeCwd(joinPaths(directRepoRoot, value.path));
          }
        }}
        hideRightBorder={false}
        root={directRepoRoot}
        key={directRepoRoot}
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
function SubmoduleSelector({
  submodules,
  selected,
  onChangeSelected,
  root,
  hideRightBorder = true,
}: {
  submodules: ReadonlyArray<Submodule>;
  selected?: Submodule;
  onChangeSelected: (newSelected: Submodule) => unknown;
  root: AbsolutePath;
  hideRightBorder?: boolean;
}) {
  const selectedValue = submodules.find(m => m.path === selected?.path)?.path;
  const [query, setQuery] = useState('');
  const toDisplay = submodules
    .filter(m => m.active && m.name.toLowerCase().includes(query.toLowerCase()))
    .sort((a, b) => a.name.localeCompare(b.name));

  return (
    <Tooltip
      trigger="click"
      placement="bottom"
      title={<SubmoduleHint path={selectedValue} root={root} />}
      component={dismiss => (
        <Column xstyle={styles.submoduleDropdownContainer}>
          <TextField
            autoFocus
            width="100%"
            placeholder={t('search submodule name')}
            value={query}
            onInput={e => setQuery(e.currentTarget?.value ?? '')}
          />
          <div {...stylex.props(styles.submoduleList)}>
            <ScrollY maxSize={360}>
              {toDisplay.map(m => (
                <div
                  key={m.path}
                  {...stylex.props(styles.submoduleOption)}
                  onClick={() => {
                    onChangeSelected(m);
                    setQuery('');
                    dismiss();
                  }}
                  title={m.path}>
                  {m.name}
                </div>
              ))}
            </ScrollY>
          </div>
        </Column>
      )}>
      <Icon
        icon="chevron-right"
        {...stylex.props(
          buttonStyles.icon,
          styles.submoduleSeparator,
          styles.hideLeftBorder,
          styles.hideRightBorder,
        )}
      />
      <Button
        {...stylex.props(
          buttonStyles.button,
          buttonStyles.icon,
          styles.submoduleSelect,
          styles.hideLeftBorder,
          hideRightBorder && styles.hideRightBorder,
        )}>
        {selected ? selected.name : `${t('submodules')}...`}
      </Button>
    </Tooltip>
  );
}

function SubmoduleHint({path, root}: {path: RepoRelativePath | undefined; root: AbsolutePath}) {
  return (
    <T>{path ? `${t('Submodule at')}: ${path}` : `${t('Select a submodule under')}: ${root}`}</T>
  );
}
