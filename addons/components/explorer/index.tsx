/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {ViewportOverlayRoot} from '../ViewportOverlay';
import {ComponentExplorer} from './ComponentExplorer';
import 'react';
import ReactDOM from 'react-dom/client';

import '../theme/index.css';
import '../theme/themeDark.css';
import '../theme/themeLight.css';

// Include CSS variables we use, originally from vscode-webview-ui-toolkit
import '../theme/themeDarkVariables.css';
import '../theme/themeLightVariables.css';

// eslint-disable-next-line @typescript-eslint/no-non-null-assertion
const root = ReactDOM.createRoot(document.getElementById('root')!);
root.render(
  <div className="light-theme">
    <ComponentExplorer />
    <ViewportOverlayRoot />
  </div>,
);
