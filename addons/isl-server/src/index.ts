/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Logger} from './logger';
import type {ServerPlatform} from './serverPlatform';

import {repositoryCache} from './RepositoryCache';
import ServerToClientAPI from './ServerToClientAPI';
import {fileLogger, stdoutLogger} from './logger';

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
  onDidReceiveMessage(hander: (event: Buffer) => void | Promise<void>): {dispose(): void};

  /**
   * Which command to use to run `sl`
   */
  command?: string;
  logFileLocation?: string;
  logger?: Logger;
  cwd: string;

  platform?: ServerPlatform;
}

export function onClientConnection(connection: ClientConnection): () => void {
  const logger =
    connection.logger ??
    (connection.logFileLocation ? fileLogger(connection.logFileLocation) : stdoutLogger);
  connection.logger = logger;
  const command = connection?.command ?? 'sl';
  logger.log(`establish ${command} client connection for ${connection.cwd}`);

  // start listening to messages
  let api: ServerToClientAPI | null = new ServerToClientAPI(connection);

  const repositoryReference = repositoryCache.getOrCreate(command, logger, connection.cwd);
  repositoryReference.promise.then(repoOrError => {
    api?.setCurrentRepoOrError(repoOrError, connection.cwd);
  });

  return () => {
    repositoryReference.unref();
    api?.dispose();
    api = null;
  };
}
