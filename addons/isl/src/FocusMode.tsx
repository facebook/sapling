/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Column} from './ComponentUtils';
import {useCommand} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {Tooltip} from './Tooltip';
import {tracker} from './analytics';
import {focusMode} from './atoms/FocusModeState';
import {Button} from './components/Button';
import {T} from './i18n';
import {colors} from './tokens.stylex';
import * as stylex from '@stylexjs/stylex';
import {useAtom} from 'jotai';
import {Icon} from 'shared/Icon';
import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';

const styles = stylex.create({
  focused: {
    backgroundColor: colors.blue,
    color: 'white',
  },
});

export function FocusModeToggle() {
  const [focused, setFocused] = useAtom(focusMode);

  const toggleFocus = () => {
    const shouldFocus = !focused;
    tracker.track('SetFocusMode', {extras: {focus: shouldFocus}});
    setFocused(shouldFocus);
  };

  useCommand('ToggleFocusMode', toggleFocus);

  const shortcut = <Kbd keycode={KeyCode.F} modifiers={[Modifier.ALT]} />;
  return (
    <Tooltip
      placement="bottom"
      title={
        <Column style={{alignItems: 'flex-start'}}>
          <div>
            {focused ? (
              <T replace={{$shortcut: shortcut}}>
                Focus Mode is enabled. Click to disable. ($shortcut)
              </T>
            ) : (
              <T replace={{$shortcut: shortcut}}>Click to enable Focus Mode. ($shortcut)</T>
            )}
          </div>
          <T>In Focus Mode, commits outside your current stack are hidden.</T>
        </Column>
      }>
      <Button
        icon
        xstyle={focused && styles.focused}
        onClick={toggleFocus}
        data-focus-mode={focused}
        data-testid="focus-mode-toggle">
        <Icon icon={focused ? 'screen-normal' : 'screen-full'} />
      </Button>
    </Tooltip>
  );
}
