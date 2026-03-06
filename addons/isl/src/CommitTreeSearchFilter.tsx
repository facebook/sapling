/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as stylex from '@stylexjs/stylex';
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
  input: {
    minWidth: '80px',
    width: '150px',
  },
});

export function CommitTreeSearchFilterInput() {
  const [filter, setFilter] = useAtom(commitTreeSearchFilter);
  return (
    <div {...stylex.props(styles.container)}>
      <Icon icon="search" />
      <TextField
        xstyle={styles.input}
        placeholder={t('Filter commits...')}
        value={filter}
        onInput={e => setFilter(e.currentTarget?.value ?? '')}
        data-testid="commit-tree-search-filter"
      />
    </div>
  );
}
