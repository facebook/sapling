/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from 'isl-server/src/logger';

import * as packageJson from '../package.json';
import {registerSaplingDiffContentProvider} from './DiffContentProvider';
import {watchAndCreateRepositoriesForWorkspaceFolders} from './VSCodeRepo';
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
    context.subscriptions.push(watchAndCreateRepositoriesForWorkspaceFolders(logger));
    context.subscriptions.push(registerSaplingDiffContentProvider(logger));
    context.subscriptions.push(...registerCommands(extensionTracker));
    extensionTracker.track('VSCodeExtensionActivated', {duration: Date.now() - start});
  } catch (error) {
    extensionTracker.error('VSCodeExtensionActivated', 'VSCodeActivationError', error as Error, {
      duration: Date.now() - start,
    });
  }
}

function createOutputChannelLogger(): [vscode.OutputChannel, Logger] {
  const outputChannel = vscode.window.createOutputChannel('Sapling ISL');
  const log = (...data: Array<unknown>) => outputChannel.appendLine(util.format(...data));
  const outputChannelLogger = {
    log,
    info: log,
    warn: log,
    error: log,
  } as Logger;
  return [outputChannel, outputChannelLogger];
}
