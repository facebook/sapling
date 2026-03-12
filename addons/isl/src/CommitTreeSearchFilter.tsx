/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {KeyCode, Modifier} from 'isl-components/KeyboardShortcuts';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtom, useAtomValue} from 'jotai';
import {colors} from '../../components/theme/tokens.stylex';
import {DropdownFields} from './DropdownFields';
import {useCommandEvent} from './ISLShortcuts';
import {T, t} from './i18n';

export const commitTreeSearchFilter = atom<string>('');

const styles = stylex.create({
  inputContainer: {
    position: 'relative',
    display: 'flex',
    alignItems: 'center',
  },
  input: {
    minWidth: '300px',
    paddingRight: '24px',
  },
  clearButton: {
    position: 'absolute',
    right: '2px',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    cursor: 'pointer',
    background: 'none',
    border: 'none',
    padding: '2px',
    color: 'var(--foreground)',
    opacity: {
      default: 0.7,
      ':hover': 1,
    },
  },
  active: {
    backgroundColor: colors.blue,
    color: 'white',
  },

  buttonContainer: {
    position: 'relative',
    display: 'flex',
  },
});

export function CommitTreeSearchFilterButton() {
  const filter = useAtomValue(commitTreeSearchFilter);
  const additionalToggles = useCommandEvent('ToggleFilterDropdown');
  const isActive = filter !== '';

  const shortcut = <Kbd keycode={KeyCode.F} modifiers={[Modifier.CMD]} />;
  return (
    <Tooltip
      trigger="click"
      component={dismiss => <FilterDropdown dismiss={dismiss} />}
      group="topbar"
      placement="bottom"
      additionalToggles={additionalToggles.asEventTarget()}
      title={<T replace={{$shortcut: shortcut}}>Filter Commits ($shortcut)</T>}>
      <div {...stylex.props(styles.buttonContainer)}>
        <Button
          icon
          data-testid="filter-commits-button"
          {...stylex.props(isActive && styles.active)}>
          <Icon
            icon={isActive ? 'filter-filled' : 'filter'}
            {...stylex.props(isActive && styles.active)}
          />
        </Button>
      </div>
    </Tooltip>
  );
}

function FilterDropdown({dismiss: _dismiss}: {dismiss: () => void}) {
  const [filter, setFilter] = useAtom(commitTreeSearchFilter);

  return (
    <DropdownFields title={<T>Filter Commits</T>} icon="filter">
      <div {...stylex.props(styles.inputContainer)}>
        <TextField
          autoFocus
          xstyle={styles.input}
          placeholder={t('Filter by title, hash, or bookmark...')}
          value={filter}
          onInput={e => setFilter(e.currentTarget?.value ?? '')}
          data-testid="commit-tree-search-filter"
        />
        {filter !== '' && (
          <Button
            icon
            xstyle={styles.clearButton}
            onClick={() => setFilter('')}
            aria-label={t('Clear filter')}>
            <Icon icon="close" size="S" />
          </Button>
        )}
      </div>
    </DropdownFields>
  );
}
