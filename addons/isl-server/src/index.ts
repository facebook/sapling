/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from './logger';
import type {ServerPlatform} from './serverPlatform';
import type {AppMode} from 'isl/src/types';

import {Internal} from './Internal';
import ServerToClientAPI from './ServerToClientAPI';
import {makeServerSideTracker} from './analytics/serverSideTracker';
import {fileLogger, stdoutLogger} from './logger';
import {browserServerPlatform} from './serverPlatform';

export interface ClientConnection {
  /**
   * Used to send a message from the server to the client.
   *
   * Designed to match
   * https://code.visualstudio.com/api/references/vscode-api#Webview.postMessage
   */
  postMessage(message: string): Promise<boolean>;

  /**
   * Designed to match
   * https://code.visualstudio.com/api/references/vscode-api#Webview.onDidReceiveMessage
   */
  onDidReceiveMessage(hander: (event: Buffer, isBinary: boolean) => void | Promise<void>): {
    dispose(): void;
  };

  /**
   * Which command to use to run `sl`
   */
  command?: string;
  /**
   * Platform-specific version string.
   * For `sl web`, this is the `sl` version.
   * For the VS Code extension, this is the extension version.
   */
  version: string;
  logFileLocation?: string;
  logger?: Logger;
  cwd: string;

  platform?: ServerPlatform;
  appMode: AppMode;
}

export function onClientConnection(connection: ClientConnection): () => void {
  const logger =
    connection.logger ??
    (connection.logFileLocation ? fileLogger(connection.logFileLocation) : stdoutLogger);
  connection.logger = logger;
  const platform = connection?.platform ?? browserServerPlatform;
  const version = connection?.version ?? 'unknown';
  logger.log(`establish client connection for ${connection.cwd}`);
  logger.log(`platform '${platform.platformName}', version '${version}'`);
  void Internal.logInternalInfo?.(logger);

  const tracker = makeServerSideTracker(logger, platform, version);
  tracker.track('ClientConnection', {extras: {cwd: connection.cwd, appMode: connection.appMode}});

  // start listening to messages
  let api: ServerToClientAPI | null = new ServerToClientAPI(platform, connection, tracker, logger);
  api.setActiveRepoForCwd(connection.cwd);

  return () => {
    api?.dispose();
    api = null;
  };
}
