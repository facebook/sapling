/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import serverAPI from '../../isl/src/ClientToServerAPI';
import {Internal} from './Internal';
import App from 'isl/src/App';
import {getLastestOperationInfo, onOperationExited} from 'isl/src/operationsState';
import {registerDisposable} from 'isl/src/utils';
import ReactDOM from 'react-dom/client';
import './vscode-styles.css';

const PHABRICATOR_DIFF_ID_REGEX = /D([1-9][0-9]{5,})/im;

registerDisposable(
  serverAPI,
  onOperationExited((progress, operation) => {
    if (progress.exitCode !== 0) {
      return; // don't show survey if submit failed
    }
    const isJfSubmitOperation = Internal.isJfSubmitOperation;
    if (!isJfSubmitOperation || !isJfSubmitOperation(operation)) {
      return; // only show survey for submits
    }

    // get latest operation
    const info = getLastestOperationInfo(operation);

    if (info == null || info.commandOutput == null) {
      return;
    }

    // phabricator url is in the last line of the commandOutput
    const message = info.commandOutput[info.commandOutput.length - 1];

    const onCommitFormSubmit = Internal.onCommitFormSubmit;

    const match = PHABRICATOR_DIFF_ID_REGEX.exec(message);
    if (onCommitFormSubmit !== undefined) {
      if (match && match[0]) {
        onCommitFormSubmit(match[0]);
      } else {
        onCommitFormSubmit();
      }
    }
  }),
);

window.addEventListener('load', () => {
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
  const root = ReactDOM.createRoot(document.getElementById('root')!);
  root.render(<App />);
});
