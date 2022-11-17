/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ServerPlatform} from '../src/serverPlatform';
import type {PlatformName} from 'isl/src/types';
import type {AddressInfo} from 'net';

import {repositoryCache} from '../src/RepositoryCache';
import {CLOSED_AND_SHOULD_NOT_RECONNECT_CODE} from '../src/constants';
import {onClientConnection} from '../src/index';
import {areTokensEqual} from './proxyUtils';
import fs from 'fs';
import http from 'http';
import path from 'path';
import urlModule from 'url';
import WebSocket from 'ws';

const ossSmartlogDir = path.join(__dirname, '../../isl');

export type StartServerArgs = {
  port: number;
  sensitiveToken: string;
  challengeToken: string;
  logFileLocation: string;
  logInfo: (...args: Parameters<typeof console.log>) => void;
  command: string;
  foreground: boolean;
};

export type StartServerResult =
  | {type: 'addressInUse'}
  | {type: 'success'; port: number; pid: number}
  | {type: 'error'; error: string};

export type ServerChallengeResponse = {
  challengeToken: string;
  /** Process ID for the server. */
  pid: number;
};

export function startServer({
  port,
  sensitiveToken,
  challengeToken,
  logFileLocation,
  logInfo,
  command,
  foreground,
}: StartServerArgs): Promise<StartServerResult> {
  return new Promise(resolve => {
    try {
      const manifest = JSON.parse(
        fs.readFileSync(path.join(ossSmartlogDir, 'build/asset-manifest.json'), 'utf-8'),
      ) as {files: Array<string>};
      for (const file of Object.values(manifest.files)) {
        if (!file.startsWith('/')) {
          resolve({type: 'error', error: `expected entry to start with / but was: \`${file}\``});
        }

        requestUrlToResource[file] = file.slice(1);
      }
    } catch (e) {
      // ignore...
    }

    // Anything not part of the asset-manifest we need to explicitly serve
    requestUrlToResource[`/favicon.ico`] = 'favicon.ico';

    /**
     * Event listener for HTTP server "error" event.
     */
    function onError(error: {syscall?: string; code?: string}) {
      if (error.syscall !== 'listen') {
        resolve({type: 'error', error: error.toString()});
        throw error;
      }

      // handle specific listen errors with friendly messages
      switch (error.code) {
        case 'EACCES': {
          resolve({type: 'error', error: `Port ${port} requires elevated privileges`});
          throw error;
        }
        case 'EADDRINUSE': {
          resolve({type: 'addressInUse'});
          return;
        }
        default:
          resolve({type: 'error', error: error.toString()});
          throw error;
      }
    }

    /**
     * Create HTTP server.
     */
    const server = http.createServer(async (req, res) => {
      if (req.url) {
        // Only the websocket is sensitive and requires the token.
        // Normal resource requests don't need to check the token.
        const {pathname} = urlModule.parse(req.url);
        // eslint-disable-next-line no-prototype-builtins
        if (pathname != null && requestUrlToResource.hasOwnProperty(pathname)) {
          const relativePath = requestUrlToResource[pathname];
          let contents: string | Buffer;
          try {
            contents = await fs.promises.readFile(path.join(ossSmartlogDir, 'build', relativePath));
          } catch (e: unknown) {
            res.writeHead(500, {'Content-Type': 'text/plain'});
            res.end(htmlEscape((e as Error).toString()));
            return;
          }

          const lastDot = relativePath.lastIndexOf('.');
          const ext = relativePath.slice(lastDot + 1);
          const contentType = extensionToMIMEType[ext] ?? 'text/plain';

          res.writeHead(200, {'Content-Type': contentType});
          res.end(contents);
          return;
        } else if (pathname === '/challenge_authenticity') {
          // requests to /challenge_authenticity?token=... allow using the sensistive token to ask
          // for the secondary challenge token.
          const requestToken = getSearchParams(req.url).get('token');
          if (requestToken && areTokensEqual(requestToken, sensitiveToken)) {
            // they know the original token, we can tell them our challenge token
            res.writeHead(200, {'Content-Type': 'text/json'});
            const response: ServerChallengeResponse = {challengeToken, pid: process.pid};
            res.end(JSON.stringify(response));
          } else {
            res.writeHead(401, {'Content-Type': 'text/json'});
            res.end(JSON.stringify({error: 'invalid token'}));
          }
          return;
        }
      }

      res.writeHead(404, {'Content-Type': 'text/html'});
      res.end('<html><body>Not Found!</body></html>');
    });

    /**
     * Listen on provided port, on all network interfaces.
     */
    const httpServer = server.listen(port);
    const wsServer = new WebSocket.Server({noServer: true, path: '/ws'});
    wsServer.on('connection', async (socket, connectionRequest) => {
      // We require websocket connections to contain the token as a URL search parameter.
      let providedToken: string | undefined;
      let cwd: string | undefined;
      let platform: string | undefined;
      if (connectionRequest.url) {
        const searchParams = getSearchParams(connectionRequest.url);
        providedToken = searchParams.get('token');
        const cwdParam = searchParams.get('cwd');
        platform = searchParams.get('platform') as string;
        if (cwdParam) {
          cwd = decodeURIComponent(cwdParam);
        }
      }
      if (!providedToken) {
        const reason = 'No token provided in websocket request';
        logInfo('closing ws:', reason);
        socket.close(CLOSED_AND_SHOULD_NOT_RECONNECT_CODE, reason);
        return;
      }
      if (!areTokensEqual(providedToken, sensitiveToken)) {
        const reason = 'Invalid token';
        logInfo('closing ws:', reason);
        socket.close(CLOSED_AND_SHOULD_NOT_RECONNECT_CODE, reason);
        return;
      }

      let platformImpl: ServerPlatform | undefined = undefined;
      switch (platform as PlatformName) {
        case 'androidStudio':
          platformImpl = (await import('../platform/androidstudioServerPlatform')).platform;
          break;
        default:
        case undefined:
          break;
      }

      const dispose = onClientConnection({
        postMessage(message: string) {
          socket.send(message);
          return Promise.resolve(true);
        },
        onDidReceiveMessage(handler) {
          const emitter = socket.on('message', handler);
          const dispose = () => emitter.off('message', handler);
          return {dispose};
        },
        cwd: cwd ?? process.cwd(),
        logFileLocation: logFileLocation === 'stdout' ? undefined : logFileLocation,
        command,

        platform: platformImpl,
      });
      socket.on('close', () => {
        dispose();

        // After disposing, we may not have anymore servers alive anymore.
        // We can proactively clean up the server so you get the latest version next time you try.
        // This way, we only re-use servers if you keep the tab open.
        // Note: since we trigger this cleanup on dispose, if you start a server with `--no-open`,
        // it won't clean itself up until you connect at least once.
        if (!foreground) {
          // We do this on a 1-minute delay in case you close a tab and quickly re-open it.
          setTimeout(() => {
            checkIfServerShouldCleanItselfUp();
          }, 60_000);
        }
      });
    });
    httpServer.on('upgrade', (request, socket, head) => {
      wsServer.handleUpgrade(request, socket, head, socket => {
        wsServer.emit('connection', socket, request);
      });
    });

    server.on('error', onError);

    // return succesful result when the server is successfully listening
    server.on('listening', () =>
      resolve({type: 'success', port: (server.address() as AddressInfo).port, pid: process.pid}),
    );
  });
}

function checkIfServerShouldCleanItselfUp() {
  if (repositoryCache.numberOfActiveServers() === 0) {
    process.exit(0);
  }
}

function getSearchParams(url: string): Map<string, string> {
  const searchParamsArray = urlModule
    .parse(url)
    .search?.replace(/^\?/, '')
    .split('&')
    .map((pair: string): [string, string] => pair.split('=') as [string, string]);

  return new Map(searchParamsArray);
}

const extensionToMIMEType: {[key: string]: string} = {
  css: 'text/css',
  html: 'text/html',
  js: 'text/javascript',
  ttf: 'font/ttf',
};

const requestUrlToResource: {[key: string]: string} = {
  '/': 'index.html',
};

function htmlEscape(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#27;');
}
