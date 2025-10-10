/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Level} from 'isl-server/src/logger';
import type {ServerPlatform} from 'isl-server/src/serverPlatform';
import type {RepositoryContext} from 'isl-server/src/serverTypes';
import type {SaplingExtensionApi} from './api/types';
import type {EnabledSCMApiFeature} from './types';

import {makeServerSideTracker} from 'isl-server/src/analytics/serverSideTracker';
import {Logger} from 'isl-server/src/logger';
import * as util from 'node:util';
import * as vscode from 'vscode';
import {DeletedFileContentProvider} from './DeletedFileContentProvider';
import {registerSaplingDiffContentProvider} from './DiffContentProvider';
import {Internal} from './Internal';
import {VSCodeReposList} from './VSCodeRepo';
import {makeExtensionApi} from './api/api';
import {InlineBlameProvider} from './blame/blame';
import {registerCommands} from './commands';
import {getCLICommand} from './config';
import {ensureTranslationsLoaded} from './i18n';
import {registerISLCommands} from './islWebviewPanel';
import {extensionVersion} from './utils';
import {getVSCodePlatform} from './vscodePlatform';

export async function activate(
  context: vscode.ExtensionContext,
): Promise<SaplingExtensionApi | undefined> {
  const start = Date.now();
  const [outputChannel, logger] = createOutputChannelLogger();
  const platform = getVSCodePlatform(context);
  const extensionTracker = makeServerSideTracker(
    logger,
    platform as ServerPlatform,
    extensionVersion,
  );
  try {
    const ctx: RepositoryContext = {
      cmd: getCLICommand(),
      cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd(),
      logger,
      tracker: extensionTracker,
    };
    // TODO: This await is in the critical path to loading the ISL webview,
    // but none of these features really apply for the webview. Can we defer this to speed up first ISL load?
    const [, enabledSCMApiFeatures] = await Promise.all([
      ensureTranslationsLoaded(context),
      Internal.getEnabledSCMApiFeatures?.(ctx) ??
        new Set<EnabledSCMApiFeature>(['blame', 'sidebar']),
    ]);
    logger.info('enabled features: ', [...enabledSCMApiFeatures].join(', '));
    context.subscriptions.push(registerISLCommands(context, platform, logger));
    context.subscriptions.push(outputChannel);
    const reposList = new VSCodeReposList(logger, extensionTracker, enabledSCMApiFeatures);
    context.subscriptions.push(reposList);
    if (enabledSCMApiFeatures.has('blame')) {
      context.subscriptions.push(new InlineBlameProvider(reposList, ctx));
    }
    context.subscriptions.push(registerSaplingDiffContentProvider(ctx));
    context.subscriptions.push(new DeletedFileContentProvider());
    let inlineCommentsProvider;
    if (enabledSCMApiFeatures.has('comments') && Internal.inlineCommentsProvider) {
      if (
        enabledSCMApiFeatures.has('newInlineComments') &&
        Internal.registerNewInlineCommentsProvider
      ) {
        context.subscriptions.push(
          ...Internal.registerNewInlineCommentsProvider(
            context,
            extensionTracker,
            logger,
            reposList,
          ),
        );
      } else {
        inlineCommentsProvider = Internal.inlineCommentsProvider(context, reposList, ctx, []);
        if (inlineCommentsProvider != null) {
          context.subscriptions.push(inlineCommentsProvider);
        }
      }
    }
    if (Internal.SaplingISLUriHandler != null) {
      context.subscriptions.push(
        vscode.window.registerUriHandler(
          new Internal.SaplingISLUriHandler(reposList, ctx, inlineCommentsProvider),
        ),
      );
    }

    context.subscriptions.push(...registerCommands(ctx));

    Internal?.registerInternalBugLogsProvider != null &&
      context.subscriptions.push(Internal.registerInternalBugLogsProvider(logger));

    extensionTracker.track('VSCodeExtensionActivated', {duration: Date.now() - start});
    const api = makeExtensionApi(platform, ctx, reposList);
    return api;
  } catch (error) {
    extensionTracker.error('VSCodeExtensionActivated', 'VSCodeActivationError', error as Error, {
      duration: Date.now() - start,
    });
    return undefined;
  }
}

function createOutputChannelLogger(): [vscode.OutputChannel, Logger] {
  const outputChannel = vscode.window.createOutputChannel('Sapling ISL');
  const outputChannelLogger = new VSCodeOutputChannelLogger(outputChannel);
  return [outputChannel, outputChannelLogger];
}

class VSCodeOutputChannelLogger extends Logger {
  private logFileContents: Array<string> = []; // TODO: we should just move this into Logger itself... and maybe do some rotation or cap memory usage!
  constructor(private outputChannel: vscode.OutputChannel) {
    super();
  }

  write(level: Level, timeStr: string, ...args: Parameters<typeof console.log>): void {
    const str = util.format('%s%s', timeStr, this.levelToString(level), ...args);
    this.logFileContents.push(str);
    this.outputChannel.appendLine(str);
  }

  getLogFileContents() {
    return Promise.resolve(this.logFileContents.join('\n'));
  }
}
