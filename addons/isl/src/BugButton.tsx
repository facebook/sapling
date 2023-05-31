/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Copyable} from './Copyable';
import {DropdownFields} from './DropdownFields';
import {ErrorBoundary} from './ErrorNotice';
import {Internal} from './Internal';
import {Tooltip} from './Tooltip';
import {tracker} from './analytics';
import {T} from './i18n';
import platform from './platform';
import {applicationinfo} from './serverAPIState';
import {VSCodeButton, VSCodeDivider} from '@vscode/webview-ui-toolkit/react';
import {Suspense, useEffect} from 'react';
import {atom, DefaultValue, useRecoilState, useRecoilValue, useSetRecoilState} from 'recoil';
import {Icon} from 'shared/Icon';

import './BugButton.css';

export function BugButton() {
  return (
    <MaybeBugButtonNux>
      <Tooltip
        trigger="click"
        component={dismiss => <BugDropdown dismiss={dismiss} />}
        placement="bottom">
        <VSCodeButton appearance="icon" data-testid="bug-button">
          <Icon icon="bug" />
        </VSCodeButton>
      </Tooltip>
    </MaybeBugButtonNux>
  );
}

export const bugButtonNux = atom<string | null>({
  key: 'bugButtonNux',
  default: null,
  effects: [
    // track how long the nux is shown
    ({onSet}) => {
      let start: number | undefined;
      onSet((value, previousValue) => {
        if (value != null) {
          // starting to show nux
          start = Date.now();
        } else {
          // stopped showing nux by clearing value
          tracker.track('ShowBugButtonNux', {
            extras: {nux: previousValue instanceof DefaultValue ? null : previousValue},
            duration: start == null ? undefined : Date.now() - start,
          });
        }
      });
    },
  ],
});

/**
 * Allow other actions to show a new-user ("nux") tooltip on the bug icon.
 * This is useful to explain how to file a bug or opt out.
 */
function MaybeBugButtonNux({children}: {children: JSX.Element}) {
  const [nux, setNux] = useRecoilState(bugButtonNux);
  if (nux == null) {
    return children;
  }

  function Nux() {
    return (
      <div className="bug-button-nux">
        {nux}
        <VSCodeButton appearance="icon" onClick={() => setNux(null)}>
          <Icon icon="x" />
        </VSCodeButton>
      </div>
    );
  }
  return (
    <Tooltip trigger="manual" shouldShow component={Nux} placement="bottom">
      {children}
    </Tooltip>
  );
}

function BugDropdown({dismiss}: {dismiss: () => void}) {
  const info = useRecoilValue(applicationinfo);

  const setBugButtonNux = useSetRecoilState(bugButtonNux);
  useEffect(() => {
    // unset nux if you open the bug menu
    setBugButtonNux(null);
  }, [setBugButtonNux]);

  const AdditionalDebugContent = platform.AdditionalDebugContent;
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
        {AdditionalDebugContent && (
          <div className="additional-debug-content">
            <VSCodeDivider />
            <ErrorBoundary>
              <Suspense>
                <AdditionalDebugContent />
              </Suspense>
            </ErrorBoundary>
          </div>
        )}
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
