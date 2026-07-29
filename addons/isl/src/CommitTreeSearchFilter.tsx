/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {KeyCode} from 'isl-components/KeyboardShortcuts';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtom, useAtomValue} from 'jotai';
import css from './CommitTreeSearchFilter.module.css';
import {DropdownFields} from './DropdownFields';
import {CMD, useCommandEvent} from './ISLShortcuts';
import {T, t} from './i18n';

export const commitTreeSearchFilter = atom<string>('');

export function CommitTreeSearchFilterButton() {
  const filter = useAtomValue(commitTreeSearchFilter);
  const additionalToggles = useCommandEvent('ToggleFilterDropdown');
  const isActive = filter !== '';

  const shortcut = <Kbd keycode={KeyCode.F} modifiers={[CMD]} />;
  return (
    <Tooltip
      trigger="click"
      component={dismiss => <FilterDropdown dismiss={dismiss} />}
      group="topbar"
      placement="bottom"
      additionalToggles={additionalToggles.asEventTarget()}
      title={<T replace={{$shortcut: shortcut}}>Filter Commits ($shortcut)</T>}>
      <div className={css.buttonContainer}>
        <Button
          icon
          data-testid="filter-commits-button"
          className={isActive ? css.active : undefined}>
          <Icon
            icon={isActive ? 'filter-filled' : 'filter'}
            className={isActive ? css.active : undefined}
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
      <div className={css.inputContainer}>
        <TextField
          autoFocus
          className={css.input}
          placeholder={t('Filter by title, hash, or bookmark...')}
          value={filter}
          onInput={e => setFilter(e.currentTarget?.value ?? '')}
          data-testid="commit-tree-search-filter"
        />
        {filter !== '' && (
          <Button
            icon
            className={css.clearButton}
            onClick={() => setFilter('')}
            aria-label={t('Clear filter')}>
            <Icon icon="close" size="S" />
          </Button>
        )}
      </div>
    </DropdownFields>
  );
}
