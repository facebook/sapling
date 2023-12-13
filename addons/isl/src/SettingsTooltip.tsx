/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ThemeColor} from './theme';
import type {PreferredSubmitCommand} from './types';
import type {ReactNode} from 'react';

import {confirmShouldSubmitEnabledAtom} from './ConfirmSubmitStack';
import {DropdownField, DropdownFields} from './DropdownFields';
import {useShowKeyboardShortcutsHelp} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {RestackBehaviorSetting} from './RestackBehavior';
import {Subtle} from './Subtle';
import {Tooltip} from './Tooltip';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {showDiffNumberConfig} from './codeReview/DiffBadge';
import {SubmitAsDraftCheckbox} from './codeReview/DraftCheckbox';
import {debugToolsEnabledState} from './debug/DebugToolsState';
import {t, T} from './i18n';
import {SetConfigOperation} from './operations/SetConfigOperation';
import platform from './platform';
import {renderCompactAtom, useZoomShortcut, zoomUISettingAtom} from './responsive';
import {repositoryInfo, useRunOperation} from './serverAPIState';
import {useThemeShortcut, themeState} from './theme';
import {
  VSCodeButton,
  VSCodeCheckbox,
  VSCodeDropdown,
  VSCodeLink,
  VSCodeOption,
} from '@vscode/webview-ui-toolkit/react';
import {useRecoilState, useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';
import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';
import {unwrap} from 'shared/utils';

import './VSCodeDropdown.css';
import './SettingsTooltip.css';

export function SettingsGearButton() {
  useThemeShortcut();
  useZoomShortcut();
  const showShortcutsHelp = useShowKeyboardShortcutsHelp();
  return (
    <Tooltip
      trigger="click"
      component={dismiss => (
        <SettingsDropdown dismiss={dismiss} showShortcutsHelp={showShortcutsHelp} />
      )}
      placement="bottom">
      <VSCodeButton appearance="icon" data-testid="settings-gear-button">
        <Icon icon="gear" />
      </VSCodeButton>
    </Tooltip>
  );
}

function SettingsDropdown({
  dismiss,
  showShortcutsHelp,
}: {
  dismiss: () => unknown;
  showShortcutsHelp: () => unknown;
}) {
  const [theme, setTheme] = useRecoilState(themeState);
  const [repoInfo, setRepoInfo] = useRecoilState(repositoryInfo);
  const runOperation = useRunOperation();
  const [showDiffNumber, setShowDiffNumber] = useRecoilState(showDiffNumberConfig);
  return (
    <DropdownFields title={<T>Settings</T>} icon="gear" data-testid="settings-dropdown">
      <VSCodeButton
        appearance="icon"
        onClick={() => {
          dismiss();
          showShortcutsHelp();
        }}>
        <T
          replace={{
            $shortcut: <Kbd keycode={KeyCode.QuestionMark} modifiers={[Modifier.SHIFT]} />,
          }}>
          View Keyboard Shortcuts - $shortcut
        </T>
      </VSCodeButton>
      {platform.theme != null ? null : (
        <Setting title={<T>Theme</T>}>
          <VSCodeDropdown
            value={theme}
            onChange={event =>
              setTheme(
                (event as React.FormEvent<HTMLSelectElement>).currentTarget.value as ThemeColor,
              )
            }>
            <VSCodeOption value="dark">
              <T>Dark</T>
            </VSCodeOption>
            <VSCodeOption value="light">
              <T>Light</T>
            </VSCodeOption>
          </VSCodeDropdown>
          <div style={{marginTop: 'var(--pad)'}}>
            <Subtle>
              <T>Toggle: </T>
              <Kbd keycode={KeyCode.T} modifiers={[Modifier.ALT]} />
            </Subtle>
          </div>
        </Setting>
      )}

      <Setting title={<T>UI Scale</T>}>
        <ZoomUISetting />
      </Setting>
      <Setting title={<T>Commits</T>}>
        <RenderCompactSetting />
      </Setting>
      <Setting title={<T>Conflicts</T>}>
        <RestackBehaviorSetting />
      </Setting>
      {/* TODO: enable this setting when there is actually a chocie to be made here. */}
      {/* <Setting
        title={<T>Language</T>}
        description={<T>Locale for translations used in the UI. Currently only en supported.</T>}>
        <VSCodeDropdown value="en" disabled>
          <VSCodeOption value="en">en</VSCodeOption>
        </VSCodeDropdown>
      </Setting> */}
      {repoInfo?.type !== 'success' ? (
        <Icon icon="loading" />
      ) : repoInfo?.codeReviewSystem.type === 'github' ? (
        <Setting
          title={<T>Preferred Code Review Submit Command</T>}
          description={
            <>
              <T>Which command to use to submit code for code review on GitHub.</T>{' '}
              <VSCodeLink
                href="https://sapling-scm.com/docs/git/intro#pull-requests"
                target="_blank">
                <T>Learn More.</T>
              </VSCodeLink>
            </>
          }>
          <VSCodeDropdown
            value={repoInfo.preferredSubmitCommand ?? 'not set'}
            onChange={event => {
              const value = (event as React.FormEvent<HTMLSelectElement>).currentTarget.value as
                | PreferredSubmitCommand
                | 'not set';
              if (value === 'not set') {
                return;
              }

              runOperation(
                new SetConfigOperation('local', 'github.preferred_submit_command', value),
              );
              setRepoInfo(info => ({...unwrap(info), preferredSubmitCommand: value}));
            }}>
            {repoInfo.preferredSubmitCommand == null ? (
              <VSCodeOption value={'not set'}>(not set)</VSCodeOption>
            ) : null}
            <VSCodeOption value="ghstack">sl ghstack</VSCodeOption>
            <VSCodeOption value="pr">sl pr</VSCodeOption>
          </VSCodeDropdown>
        </Setting>
      ) : null}
      <Setting title={<T>Code Review</T>}>
        <div className="multiple-settings">
          <VSCodeCheckbox
            checked={showDiffNumber}
            onChange={e => {
              setShowDiffNumber((e.target as HTMLInputElement).checked);
            }}>
            <T>Show copyable Diff / Pull Request numbers inline for each commit</T>
          </VSCodeCheckbox>
          <ConfirmSubmitStackSetting />
          <SubmitAsDraftCheckbox forceShow />
        </div>
      </Setting>
      <DebugToolsField />
    </DropdownFields>
  );
}

function ConfirmSubmitStackSetting() {
  const [value, setValue] = useRecoilState(confirmShouldSubmitEnabledAtom);
  const provider = useRecoilValue(codeReviewProvider);
  if (provider == null || !provider.supportSubmittingAsDraft) {
    return null;
  }
  return (
    <Tooltip
      title={t(
        'This lets you choose to submit as draft and provide an update message. ' +
          'If false, no confirmation is shown and it will submit as draft if you previously ' +
          'checked the submit as draft checkbox.',
      )}>
      <VSCodeCheckbox
        checked={value}
        onChange={e => {
          setValue((e.target as HTMLInputElement).checked);
        }}>
        <T>Show confirmation when submitting a stack</T>
      </VSCodeCheckbox>
    </Tooltip>
  );
}

function RenderCompactSetting() {
  const [value, setValue] = useRecoilState(renderCompactAtom);
  return (
    <Tooltip
      title={t(
        'Render commits in the tree more compactly, by reducing spacing and not wrapping Diff info to multiple lines. ' +
          'May require more horizontal scrolling.',
      )}>
      <VSCodeCheckbox
        checked={value}
        onChange={e => {
          setValue((e.target as HTMLInputElement).checked);
        }}>
        <T>Compact Mode</T>
      </VSCodeCheckbox>
    </Tooltip>
  );
}

function ZoomUISetting() {
  const [zoom, setZoom] = useRecoilState(zoomUISettingAtom);
  function roundToPercent(n: number): number {
    return Math.round(n * 100) / 100;
  }
  return (
    <div className="zoom-setting">
      <Tooltip title={t('Decrease UI Zoom')}>
        <VSCodeButton
          className="zoom-out"
          appearance="icon"
          onClick={() => {
            setZoom(zoom => roundToPercent(zoom - 0.1));
          }}>
          <Icon icon="zoom-out" />
        </VSCodeButton>
      </Tooltip>
      <span>{`${Math.round(100 * zoom)}%`}</span>
      <Tooltip title={t('Increase UI Zoom')}>
        <VSCodeButton
          className="zoom-in"
          appearance="icon"
          onClick={() => {
            setZoom(zoom => roundToPercent(zoom + 0.1));
          }}>
          <Icon icon="zoom-in" />
        </VSCodeButton>
      </Tooltip>
      <div style={{width: '20px'}} />
      <label>
        <T>Presets:</T>
      </label>
      <VSCodeButton
        className="zoom-80"
        appearance="icon"
        onClick={() => {
          setZoom(0.8);
        }}>
        <T>Small</T>
      </VSCodeButton>
      <VSCodeButton
        className="zoom-100"
        appearance="icon"
        onClick={() => {
          setZoom(1.0);
        }}>
        <T>Normal</T>
      </VSCodeButton>
      <VSCodeButton
        className="zoom-120"
        appearance="icon"
        onClick={() => {
          setZoom(1.2);
        }}>
        <T>Large</T>
      </VSCodeButton>
    </div>
  );
}

function DebugToolsField() {
  const [isDebug, setIsDebug] = useRecoilState(debugToolsEnabledState);

  return (
    <DropdownField title={t('Debug Tools')}>
      <VSCodeCheckbox
        checked={isDebug}
        onChange={e => {
          setIsDebug((e.target as HTMLInputElement).checked);
        }}>
        <T>Enable Debug Tools</T>
      </VSCodeCheckbox>
    </DropdownField>
  );
}

function Setting({
  children,
  title,
  description,
}: {
  children: ReactNode;
  title: ReactNode;
  description?: ReactNode;
}) {
  return (
    <DropdownField title={title}>
      {description && <div className="setting-description">{description}</div>}
      {children}
    </DropdownField>
  );
}
