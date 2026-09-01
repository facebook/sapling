/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Atom} from 'jotai';

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {KeyCode} from 'isl-components/KeyboardShortcuts';
import {TextField} from 'isl-components/TextField';
import {Tooltip} from 'isl-components/Tooltip';
import {atom, useAtom, useAtomValue} from 'jotai';
import {useEffect, useRef} from 'react';
import {debounce} from 'shared/debounce';
import css from './CommitTreeSearchFilter.module.css';
import {DropdownFields} from './DropdownFields';
import {CMD, useCommandEvent} from './ISLShortcuts';
import {T, t} from './i18n';
import {atomWithOnChange, writeAtom} from './jotaiUtils';

/** How long typing has to pause before the commit graph is re-filtered. */
const FILTER_DEBOUNCE_MS = 150;

const appliedFilter = atom<string>('');

const applyFilter = (value: string) => writeAtom(appliedFilter, value);
const applyFilterWhenTypingStops = debounce(applyFilter, FILTER_DEBOUNCE_MS);

/**
 * What the filter box contains. Updates on every keystroke.
 *
 * Applying a filter walks the whole dag, so a fast typist would otherwise pay for one
 * full-graph pass per character. Anything expensive should read
 * `appliedCommitTreeSearchFilter` instead; this atom is for the box itself and for
 * "is a filter set?" checks that must feel instant.
 */
export const commitTreeSearchFilter = atomWithOnChange(
  atom<string>(''),
  value => {
    if (value === '') {
      // Clearing is one deliberate action rather than a burst, so there is nothing to
      // coalesce. Cancel the pending keystrokes so they cannot re-apply a filter the
      // user just dismissed, and bring the full graph back right away.
      applyFilterWhenTypingStops.reset();
      applyFilter(value);
    } else {
      applyFilterWhenTypingStops(value);
    }
  },
  /* skipInitialCall */ true, // Both atoms already start empty.
);

/**
 * `commitTreeSearchFilter`, lagging by up to {@link FILTER_DEBOUNCE_MS} while typing.
 * Both atoms settle on the same value; this one just gets there later.
 *
 * Read-only on purpose: the `onChange` above is the only thing that should write it, so the
 * two can only ever disagree about timing.
 */
export const appliedCommitTreeSearchFilter: Atom<string> = appliedFilter;

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
  const inputRef = useRef<HTMLInputElement | null>(null);
  // On open, select the existing query (caret at the end) so typing replaces
  // it and Delete clears it. Mount-only: refocusing within an open dropdown
  // should not re-select.
  useEffect(() => {
    const input = inputRef.current;
    input?.setSelectionRange(0, input.value.length, 'forward');
  }, []);

  return (
    <DropdownFields title={<T>Filter Commits</T>} icon="filter">
      <div className={css.inputContainer}>
        <TextField
          autoFocus
          ref={inputRef}
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
