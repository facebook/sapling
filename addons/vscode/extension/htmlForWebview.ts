/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import * as crypto from 'crypto';
import * as vscode from 'vscode';

export const devPort = 3015;
export const devUri = `http://localhost:${devPort}`;

export function getWebviewOptions(
  context: vscode.ExtensionContext,
  distFolder: string,
): vscode.WebviewOptions & vscode.WebviewPanelOptions {
  return {
    enableScripts: true,
    retainContextWhenHidden: true,
    // Restrict the webview to only loading content from our extension's output directory.
    localResourceRoots: [
      vscode.Uri.joinPath(context.extensionUri, distFolder),
      vscode.Uri.parse(devUri),
    ],
    portMapping: [{webviewPort: devPort, extensionHostPort: devPort}],
  };
}

/**
 * Get any extra styles to inject into the webview.
 * Important: this is injected into the HTML directly, and should not
 * use any user-controlled data that could be used maliciously.
 */
function getVSCodeCompatibilityStyles(): string {
  const globalStyles = new Map();

  const fontFeatureSettings = vscode.workspace
    .getConfiguration('editor')
    .get<string | boolean>('fontLigatures');
  const validFontFeaturesRegex = /^[0-9a-zA-Z"',\-_ ]*$/;
  if (fontFeatureSettings === true) {
    // no need to specify specific additional settings
  } else if (
    !fontFeatureSettings ||
    typeof fontFeatureSettings !== 'string' ||
    !validFontFeaturesRegex.test(fontFeatureSettings)
  ) {
    globalStyles.set('font-variant-ligatures', 'none');
  } else {
    globalStyles.set('font-feature-settings', fontFeatureSettings);
  }

  const tabSizeSettings = vscode.workspace.getConfiguration('editor').get<number>('tabSize');
  if (typeof tabSizeSettings === 'number') {
    globalStyles.set('--tab-size', tabSizeSettings);
  }

  const globalStylesFlat = Array.from(globalStyles, ([k, v]) => `${k}: ${v};`);
  return `
  html {
    ${globalStylesFlat.join('\n')};
  }`;
}

/**
 * When built in dev mode using vite, files are not written to disk.
 * In order to get files to load, we need to set up the server path ourself.
 *
 * Note: no CSPs in dev mode. This should not be used in production!
 */
function devModeHtmlForWebview(
  /**
   * CSS to inject into the HTML in a <style> tag
   */
  extraStyles: string,
  /**
   * javascript to inject into the HTML in a <script> tag
   * IMPORTANT: this MUST be sanitized to avoid XSS attacks
   */
  initialScript: (nonce: string) => string,
  devModeScripts: Array<string>,
  rootClass: string,
  placeholderHtml?: string,
) {
  return `<!DOCTYPE html>
	<html lang="en">
	<head>
		<meta charset="UTF-8">
		<meta name="viewport" content="width=device-width, initial-scale=1.0">
		<base href="${vscode.Uri.parse(devUri)}">

    <!-- Hot reloading code from Vite. Normally, vite injects this into the HTML.
    But since we have to load this statically, we insert it manually here.
    See https://github.com/vitejs/vite/blob/734a9e3a4b9a0824a5ba4a5420f9e1176ce74093/docs/guide/backend-integration.md?plain=1#L50-L56 -->
    <script type="module">
      import RefreshRuntime from "/@react-refresh"
      RefreshRuntime.injectIntoGlobalHook(window)
      window.$RefreshReg$ = () => {}
      window.$RefreshSig$ = () => (type) => type
      window.__vite_plugin_react_preamble_installed__ = true
    </script>
    <script type="module" src="/@vite/client"></script>
    <style>
        ${getVSCodeCompatibilityStyles()}
        ${extraStyles}
    </style>
    ${initialScript('')}
    ${devModeScripts.map(script => `<script type="module" src="${script}"></script>`).join('\n')}
	</head>
	<body>
		<div id="root" class="${rootClass}">
      ${placeholderHtml ?? 'loading (dev mode)'}
    </div>
	</body>
	</html>`;
}

const IS_DEV_BUILD = process.env.NODE_ENV === 'development';
export function htmlForWebview({
  webview,
  context,
  extraStyles,
  initialScript,
  title,
  rootClass,
  extensionRelativeBase,
  entryPointFile,
  cssEntryPointFile,
  devModeScripts,
  placeholderHtml,
}: {
  webview: vscode.Webview;
  context: vscode.ExtensionContext;
  /**
   * CSS to inject into the HTML in a <style> tag
   */
  extraStyles: string;
  /**
   * javascript to inject into the HTML in a <script> tag
   * IMPORTANT: this MUST be sanitized to avoid XSS attacks
   */
  initialScript: (nonce: string) => string;
  /** <head>'s <title> of the webview */
  title: string;
  /** className to apply to the root <div> */
  rootClass: string;
  /** Base directory the webview loads from, where `/` in HTTP requests is relative to */
  extensionRelativeBase: string;
  /** Built entry point .js javascript file name to load, relative to extensionRelativeBase */
  entryPointFile: string;
  /** Built bundle .css file name to load, relative to extensionRelativeBase */
  cssEntryPointFile: string;
  /** Entry point scripts used in dev mode, needed for hot reloading */
  devModeScripts: Array<string>;
  /** Placeholder HTML element to show while the webview is loading */
  placeholderHtml?: string;
}) {
  // Only allow accessing resources relative to webview dir,
  // and make paths relative to here.
  const baseUri = webview.asWebviewUri(
    vscode.Uri.joinPath(context.extensionUri, extensionRelativeBase),
  );

  if (IS_DEV_BUILD) {
    return devModeHtmlForWebview(
      extraStyles,
      initialScript,
      devModeScripts,
      rootClass,
      placeholderHtml,
    );
  }

  const scriptUri = entryPointFile;

  // Use a nonce to only allow specific scripts to be run
  const nonce = getNonce();

  const CSP = [
    `default-src ${webview.cspSource}`,
    `style-src ${webview.cspSource} 'unsafe-inline'`,
    // vscode-webview-ui needs to use style-src-elem without the nonce
    `style-src-elem ${webview.cspSource} 'unsafe-inline'`,
    `font-src ${webview.cspSource} data:`,
    `img-src ${webview.cspSource} https: data:`,
    `script-src ${webview.cspSource} 'nonce-${nonce}' 'wasm-unsafe-eval'`,
    `script-src-elem ${webview.cspSource} 'nonce-${nonce}'`,
    `worker-src ${webview.cspSource} 'nonce-${nonce}' blob:`,
  ].join('; ');

  return `<!DOCTYPE html>
	<html lang="en">
	<head>
		<meta charset="UTF-8">
		<meta http-equiv="Content-Security-Policy" content="${CSP}">
		<meta name="viewport" content="width=device-width, initial-scale=1.0">
		<base href="${baseUri}/">
		<title>${title}</title>

		<link href="${cssEntryPointFile}" rel="stylesheet">
		<link href="res/stylex.css" rel="stylesheet">
    <style>
        ${getVSCodeCompatibilityStyles()}
        ${extraStyles}
    </style>
    ${initialScript(nonce)}
		<script type="module" defer="defer" nonce="${nonce}" src="${scriptUri}"></script>
	</head>
	<body>
		<div id="root" class="${rootClass}">
      ${placeholderHtml ?? 'loading...'}
    </div>
	</body>
	</html>`;
}

function getNonce(): string {
  return crypto.randomBytes(16).toString('base64');
}
