/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import App from './App';
import 'react';
import ReactDOM from 'react-dom/client';

// @vscode/webview-ui-toolkit doesn't ship with light theme variables,
// we need to include them ourselves in non-vscode renders of <App />.
import './themeLightVariables.css';

// eslint-disable-next-line @typescript-eslint/no-non-null-assertion
const root = ReactDOM.createRoot(document.getElementById('root')!);
root.render(<App />);
