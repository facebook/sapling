/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ThemeColor} from './theme';
import type {PreferredSubmitCommand} from './types';
import type {ReactNode} from 'react';

import {Icon} from './Icon';
import {Tooltip} from './Tooltip';
import {repositoryInfo} from './codeReview/CodeReviewInfo';
import {T} from './i18n';
import {SetConfigOperation} from './operations/SetConfigOperation';
import platform from './platform';
import {useRunOperation} from './serverAPIState';
import {themeState} from './theme';
import {
  VSCodeButton,
  VSCodeDivider,
  VSCodeDropdown,
  VSCodeLink,
  VSCodeOption,
} from '@vscode/webview-ui-toolkit/react';
import {useRecoilState} from 'recoil';
import {unwrap} from 'shared/utils';

import './SettingsTooltip.css';

export function SettingsGearButton() {
  return (
    <Tooltip trigger="click" component={SettingsDropdown} placement="bottom">
      <VSCodeButton appearance="icon" data-testid="settings-gear-button">
        <Icon icon="gear" />
      </VSCodeButton>
    </Tooltip>
  );
}

function SettingsDropdown() {
  const [theme, setTheme] = useRecoilState(themeState);
  const [repoInfo, setRepoInfo] = useRecoilState(repositoryInfo);
  const runOperation = useRunOperation();
  return (
    <div className="settings-dropdown" data-testid="settings-dropdown">
      <div className="settings-header">
        <Icon icon="gear" size="M" />
        <div className="settings-heading" role="heading">
          <T>Settings</T>
        </div>
      </div>
      <VSCodeDivider />
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
        </Setting>
      )}
      <Setting
        title={<T>Language</T>}
        description={<T>Locale for translations used in the UI. Currently only en supported.</T>}>
        <VSCodeDropdown value="en" disabled>
          <VSCodeOption value="en">en</VSCodeOption>
        </VSCodeDropdown>
      </Setting>
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
    </div>
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
    <div className="setting">
      <div className="setting-title">{title}</div>
      {description && <div className="setting-description">{description}</div>}
      {children}
    </div>
  );
}
