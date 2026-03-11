/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {TextField} from 'isl-components/TextField';
import {atom, useAtom} from 'jotai';
import {t} from './i18n';

export const commitTreeSearchFilter = atom<string>('');

const styles = stylex.create({
  container: {
    display: 'flex',
    alignItems: 'center',
    gap: '4px',
  },
  inputContainer: {
    position: 'relative',
    display: 'flex',
    alignItems: 'center',
  },
  input: {
    minWidth: '80px',
    width: '150px',
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
});

export function CommitTreeSearchFilterInput() {
  const [filter, setFilter] = useAtom(commitTreeSearchFilter);
  return (
    <div {...stylex.props(styles.container)}>
      <Icon icon="search" />
      <div {...stylex.props(styles.inputContainer)}>
        <TextField
          xstyle={styles.input}
          placeholder={t('Filter commits...')}
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
    </div>
  );
}
