/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from 'isl-server/src/logger';

import packageJson from '../package.json';
import {registerSaplingDiffContentProvider} from './DiffContentProvider';
import {VSCodeReposList} from './VSCodeRepo';
import {InlineBlameProvider} from './blame/blame';
import {registerCommands} from './commands';
import {ensureTranslationsLoaded} from './i18n';
import {registerISLCommands} from './islWebviewPanel';
import {VSCodePlatform} from './vscodePlatform';
import {makeServerSideTracker} from 'isl-server/src/analytics/serverSideTracker';
import * as util from 'util';
import * as vscode from 'vscode';

export async function activate(context: vscode.ExtensionContext) {
  const start = Date.now();
  const [outputChannel, logger] = createOutputChannelLogger();
  const extensionTracker = makeServerSideTracker(logger, VSCodePlatform, packageJson.version);
  try {
    await ensureTranslationsLoaded(context);
    context.subscriptions.push(registerISLCommands(context, logger));
    context.subscriptions.push(outputChannel);
    const reposList = new VSCodeReposList(logger);
    context.subscriptions.push(reposList);
    context.subscriptions.push(new InlineBlameProvider(reposList, logger, extensionTracker));
    context.subscriptions.push(registerSaplingDiffContentProvider(logger));
    context.subscriptions.push(...registerCommands(extensionTracker));
    extensionTracker.track('VSCodeExtensionActivated', {duration: Date.now() - start});
  } catch (error) {
    extensionTracker.error('VSCodeExtensionActivated', 'VSCodeActivationError', error as Error, {
      duration: Date.now() - start,
    });
  }
}

const logFileContents: Array<string> = [];
function createOutputChannelLogger(): [vscode.OutputChannel, Logger] {
  const outputChannel = vscode.window.createOutputChannel('Sapling ISL');
  const log = (...data: Array<unknown>) => {
    const line = util.format(...data);
    logFileContents.push(line);
    outputChannel.appendLine(line);
  };
  const outputChannelLogger = {
    log,
    info: log,
    warn: log,
    error: log,

    getLogFileContents() {
      return Promise.resolve(logFileContents.join('\n'));
    },
  } as Logger;
  return [outputChannel, outputChannelLogger];
}
