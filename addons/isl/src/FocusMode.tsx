/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Icon} from 'isl-components/Icon';
import {Kbd} from 'isl-components/Kbd';
import {KeyCode, Modifier} from 'isl-components/KeyboardShortcuts';
import {Tooltip} from 'isl-components/Tooltip';
import {useAtom} from 'jotai';
import {Column} from './ComponentUtils';
import css from './FocusMode.module.css';
import {useCommand} from './ISLShortcuts';
import {tracker} from './analytics';
import {focusMode} from './atoms/FocusModeState';
import {T} from './i18n';

export function FocusModeToggle() {
  const [focused, setFocused] = useAtom(focusMode);

  const toggleFocus = () => {
    const shouldFocus = !focused;
    tracker.track('SetFocusMode', {extras: {focus: shouldFocus}});
    setFocused(shouldFocus);
  };

  useCommand('ToggleFocusMode', toggleFocus);

  const shortcut = <Kbd keycode={KeyCode.F} modifiers={[Modifier.ALT, Modifier.SHIFT]} />;
  return (
    <Tooltip
      placement="bottom"
      title={
        <Column alignStart>
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
        className={focused ? css.focused : undefined}
        onClick={toggleFocus}
        data-focus-mode={focused}
        data-testid="focus-mode-toggle">
        <Icon icon={focused ? 'screen-normal' : 'screen-full'} />
      </Button>
    </Tooltip>
  );
}
