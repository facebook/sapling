/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ThemeColor} from './theme';
import type {PreferredSubmitCommand} from './types';
import type {ReactNode} from 'react';

import {Row} from './ComponentUtils';
import {confirmShouldSubmitEnabledAtom} from './ConfirmSubmitStack';
import {DropdownField, DropdownFields} from './DropdownFields';
import {useShowKeyboardShortcutsHelp} from './ISLShortcuts';
import {Kbd} from './Kbd';
import {Link} from './Link';
import {RestackBehaviorSetting} from './RestackBehavior';
import {Subtle} from './Subtle';
import {Tooltip} from './Tooltip';
import {codeReviewProvider} from './codeReview/CodeReviewInfo';
import {showDiffNumberConfig} from './codeReview/DiffBadge';
import {SubmitAsDraftCheckbox} from './codeReview/DraftCheckbox';
import {Button} from './components/Button';
import {Checkbox} from './components/Checkbox';
import {Dropdown} from './components/Dropdown';
import {debugToolsEnabledState} from './debug/DebugToolsState';
import {t, T} from './i18n';
import {configBackedAtom} from './jotaiUtils';
import {SetConfigOperation} from './operations/SetConfigOperation';
import {useRunOperation} from './operationsState';
import platform from './platform';
import {renderCompactAtom, useZoomShortcut, zoomUISettingAtom} from './responsive';
import {repositoryInfo} from './serverAPIState';
import {useThemeShortcut, themeState} from './theme';
import {useAtom, useAtomValue} from 'jotai';
import {Icon} from 'shared/Icon';
import {KeyCode, Modifier} from 'shared/KeyboardShortcuts';
import {tryJsonParse, nullthrows} from 'shared/utils';

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
      group="topbar"
      placement="bottom">
      <Button icon data-testid="settings-gear-button">
        <Icon icon="gear" />
      </Button>
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
  const [theme, setTheme] = useAtom(themeState);
  const [repoInfo, setRepoInfo] = useAtom(repositoryInfo);
  const runOperation = useRunOperation();
  const [showDiffNumber, setShowDiffNumber] = useAtom(showDiffNumberConfig);
  return (
    <DropdownFields title={<T>Settings</T>} icon="gear" data-testid="settings-dropdown">
      <Button
        style={{justifyContent: 'center', gap: 0}}
        icon
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
      </Button>
      {platform.theme != null ? null : (
        <Setting title={<T>Theme</T>}>
          <Dropdown
            options={
              [
                {value: 'light', name: 'Light'},
                {value: 'dark', name: 'Dark'},
              ] as Array<{value: ThemeColor; name: string}>
            }
            value={theme}
            onChange={event => setTheme(event.currentTarget.value as ThemeColor)}
          />
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
        <Dropdown value="en" options=['en'] />
      </Setting> */}
      {repoInfo?.type !== 'success' ? (
        <Icon icon="loading" />
      ) : repoInfo?.codeReviewSystem.type === 'github' ? (
        <Setting
          title={<T>Preferred Code Review Submit Command</T>}
          description={
            <>
              <T>Which command to use to submit code for code review on GitHub.</T>{' '}
              <Link href="https://sapling-scm.com/docs/git/intro#pull-requests">
                <T>Learn More</T>
              </Link>
            </>
          }>
          <Dropdown
            value={repoInfo.preferredSubmitCommand ?? 'not set'}
            options={(repoInfo.preferredSubmitCommand == null
              ? [{value: 'not set', name: '(not set)'}]
              : []
            ).concat([
              {value: 'ghstack', name: 'sl ghstack'},
              {value: 'pr', name: 'sl pr'},
            ])}
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
              setRepoInfo(info => ({...nullthrows(info), preferredSubmitCommand: value}));
            }}
          />
        </Setting>
      ) : null}
      <Setting title={<T>Code Review</T>}>
        <div className="multiple-settings">
          <Checkbox
            checked={showDiffNumber}
            onChange={checked => {
              setShowDiffNumber(checked);
            }}>
            <T>Show copyable Diff / Pull Request numbers inline for each commit</T>
          </Checkbox>
          <ConfirmSubmitStackSetting />
          <SubmitAsDraftCheckbox forceShow />
        </div>
      </Setting>
      {platform.canCustomizeFileOpener && (
        <Setting title={<T>Environment</T>}>
          <OpenFilesCmdSetting />
        </Setting>
      )}
      <DebugToolsField />
    </DropdownFields>
  );
}

function ConfirmSubmitStackSetting() {
  const [value, setValue] = useAtom(confirmShouldSubmitEnabledAtom);
  const provider = useAtomValue(codeReviewProvider);
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
      <Checkbox
        checked={value}
        onChange={checked => {
          setValue(checked);
        }}>
        <T>Show confirmation when submitting a stack</T>
      </Checkbox>
    </Tooltip>
  );
}

function RenderCompactSetting() {
  const [value, setValue] = useAtom(renderCompactAtom);
  return (
    <Tooltip
      title={t(
        'Render commits in the tree more compactly, by reducing spacing and not wrapping Diff info to multiple lines. ' +
          'May require more horizontal scrolling.',
      )}>
      <Checkbox
        checked={value}
        onChange={checked => {
          setValue(checked);
        }}>
        <T>Compact Mode</T>
      </Checkbox>
    </Tooltip>
  );
}

export const openFileCmdAtom = configBackedAtom<string | null>(
  'isl.open-file-cmd',
  null,
  /* readonly */ true,
  /* use raw value */ true,
);

function OpenFilesCmdSetting() {
  const cmdRaw = useAtomValue(openFileCmdAtom);
  const cmd = cmdRaw == null ? null : (tryJsonParse(cmdRaw) as string | Array<string>) ?? cmdRaw;
  const cmdEl =
    cmd == null ? (
      <T>OS Default Program</T>
    ) : (
      <code>{Array.isArray(cmd) ? cmd.join(' ') : cmd}</code>
    );
  return (
    <Tooltip
      component={() => (
        <div>
          <div>
            <T>You can configure how to open files from ISL via</T>
          </div>
          <pre>sl config --user isl.open-file-cmd "/path/to/command"</pre>
          <div>
            <T>or</T>
          </div>
          <pre>sl config --user isl.open-file-cmd '["cmd", "with", "args"]'</pre>
        </div>
      )}>
      <Row>
        <T replace={{$cmd: cmdEl}}>Open files in $cmd</T>
        <Subtle>
          <T>How to configure?</T>
        </Subtle>
        <Icon icon="question" />
      </Row>
    </Tooltip>
  );
}

function ZoomUISetting() {
  const [zoom, setZoom] = useAtom(zoomUISettingAtom);
  function roundToPercent(n: number): number {
    return Math.round(n * 100) / 100;
  }
  return (
    <div className="zoom-setting">
      <Tooltip title={t('Decrease UI Zoom')}>
        <Button
          icon
          onClick={() => {
            setZoom(roundToPercent(zoom - 0.1));
          }}>
          <Icon icon="zoom-out" />
        </Button>
      </Tooltip>
      <span>{`${Math.round(100 * zoom)}%`}</span>
      <Tooltip title={t('Increase UI Zoom')}>
        <Button
          icon
          onClick={() => {
            setZoom(roundToPercent(zoom + 0.1));
          }}>
          <Icon icon="zoom-in" />
        </Button>
      </Tooltip>
      <div style={{width: '20px'}} />
      <label>
        <T>Presets:</T>
      </label>
      <Button
        style={{fontSize: '80%'}}
        icon
        onClick={() => {
          setZoom(0.8);
        }}>
        <T>Small</T>
      </Button>
      <Button
        icon
        onClick={() => {
          setZoom(1.0);
        }}>
        <T>Normal</T>
      </Button>
      <Button
        style={{fontSize: '120%'}}
        icon
        onClick={() => {
          setZoom(1.2);
        }}>
        <T>Large</T>
      </Button>
    </div>
  );
}

function DebugToolsField() {
  const [isDebug, setIsDebug] = useAtom(debugToolsEnabledState);

  return (
    <DropdownField title={t('Debug Tools')}>
      <Checkbox
        checked={isDebug}
        onChange={checked => {
          setIsDebug(checked);
        }}>
        <T>Enable Debug Tools</T>
      </Checkbox>
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
