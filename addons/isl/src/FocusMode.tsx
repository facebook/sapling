/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Tooltip} from './Tooltip';
import {tracker} from './analytics';
import {Button} from './components/Button';
import {t} from './i18n';
import {colors} from './tokens.stylex';
import * as stylex from '@stylexjs/stylex';
import {atom, useAtom} from 'jotai';
import {Icon} from 'shared/Icon';

// Note: we intentionally don't persist focus mode, so each time you open ISL,
// you see all your commits and can choose to focus from that point onward.
export const focusMode = atom(false);

const styles = stylex.create({
  focused: {
    backgroundColor: colors.blue,
    color: 'white',
  },
});

export function FocusModeToggle() {
  const [focused, setFocused] = useAtom(focusMode);
  return (
    <Tooltip
      placement="bottom"
      title={
        (focused
          ? t('Focus Mode is enabled. Click to disable.')
          : t('Click to enable Focus Mode.')) +
        '\n' +
        t('In Focus Mode, commits outside your current stack are hidden.')
      }>
      <Button
        icon
        xstyle={focused && styles.focused}
        onClick={() => {
          const shouldFocus = !focused;
          tracker.track('SetFocusMode', {extras: {focus: shouldFocus}});
          setFocused(shouldFocus);
        }}
        data-focus-mode={focused}
        data-testid="focus-mode-toggle">
        <Icon icon={focused ? 'screen-normal' : 'screen-full'} />
      </Button>
    </Tooltip>
  );
}
