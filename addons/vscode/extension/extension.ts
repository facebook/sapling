/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from 'isl-server/src/logger';

import {registerSaplingDiffContentProvider} from './DiffContentProvider';
import {watchAndCreateRepositoriesForWorkspaceFolders} from './VSCodeRepo';
import {registerCommands} from './commands';
import {ensureTranslationsLoaded} from './i18n';
import {registerISLCommands} from './islWebviewPanel';
import * as util from 'util';
import * as vscode from 'vscode';

export async function activate(context: vscode.ExtensionContext) {
  const [outputChannel, logger] = createOutputChannelLogger();
  await ensureTranslationsLoaded(context);
  context.subscriptions.push(registerISLCommands(context, logger));
  context.subscriptions.push(outputChannel);
  context.subscriptions.push(watchAndCreateRepositoriesForWorkspaceFolders(logger));
  context.subscriptions.push(registerSaplingDiffContentProvider(logger));
  context.subscriptions.push(...registerCommands());
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
