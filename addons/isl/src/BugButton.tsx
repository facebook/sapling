/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Copyable} from './Copyable';
import {DropdownFields} from './DropdownFields';
import {Internal} from './Internal';
import {Tooltip} from './Tooltip';
import {T} from './i18n';
import platform from './platform';
import {applicationinfo} from './serverAPIState';
import {VSCodeButton} from '@vscode/webview-ui-toolkit/react';
import {useRecoilValue} from 'recoil';
import {Icon} from 'shared/Icon';

import './BugButton.css';

export function BugButton() {
  return (
    <Tooltip trigger="click" component={BugDropdown} placement="bottom">
      <VSCodeButton appearance="icon" data-testid="bug-button">
        <Icon icon="bug" />
      </VSCodeButton>
    </Tooltip>
  );
}

function BugDropdown({dismiss}: {dismiss: () => void}) {
  const info = useRecoilValue(applicationinfo);
  return (
    <DropdownFields
      title={<T>Help</T>}
      icon="bug"
      data-testid="bug-dropdown"
      className="bug-dropdown">
      {info == null ? (
        <Icon icon="loading" />
      ) : (
        <div className="bug-dropdown-version">
          <Copyable children={`ISL version ${info.version} (${info.platformName})`} />
        </div>
      )}
      <div className="bug-dropdown-actions">
        <VSCodeButton
          appearance="secondary"
          onClick={() => {
            platform.openExternalLink('https://sapling-scm.com/docs/addons/isl');
          }}>
          <Icon icon="book" slot="start" />
          <T>View Documentation</T>
        </VSCodeButton>
        <FileABug dismissBugDropdown={dismiss} />
      </div>
      {/*
      // TODO: enable these debug actions
      <div className="bug-dropdown-debug-actions">
        <VSCodeButton
          appearance="icon"
          onClick={() => {
            // TODO: platform-specific log file action
          }}>
          <Icon icon="go-to-file" slot="start" />

          <T>Reveal log file</T>
        </VSCodeButton>
        <VSCodeButton
          appearance="icon"
          onClick={() => {
            // TODO: pull all recoil state
          }}>
          <Icon icon="copy" slot="start" />
          <T>Copy UI debug information</T>
        </VSCodeButton>
      </div> */}
    </DropdownFields>
  );
}

function FileABug({dismissBugDropdown}: {dismissBugDropdown: () => void}) {
  return Internal.FileABugButton != null ? (
    <Internal.FileABugButton dismissBugDropdown={dismissBugDropdown} />
  ) : (
    <OSSFileABug />
  );
}

function OSSFileABug() {
  return (
    <>
      <VSCodeButton
        appearance="secondary"
        onClick={() => {
          platform.openExternalLink('https://discord.gg/X6baZ94Vzh');
        }}>
        <Icon icon="comment-discussion" slot="start" />
        <T>Help and Feedback on Discord</T>
      </VSCodeButton>
      <VSCodeButton
        appearance="secondary"
        onClick={() => {
          platform.openExternalLink('https://github.com/facebook/sapling/issues');
        }}>
        <Icon icon="bug" slot="start" />
        <T>Report an Issue on GitHub</T>
      </VSCodeButton>
    </>
  );
}
