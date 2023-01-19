/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ChildProcessResponse} from './child';
import type {StartServerArgs, StartServerResult} from './server';
import type {IOType} from 'child_process';
import type {PlatformName} from 'isl/src/types';

import {
  ensureExistingServerFolder,
  deleteExistingServerFile,
  writeExistingServerFile,
} from './existingServerStateFiles';
import * as lifecycle from './serverLifecycle';
import child_process from 'child_process';
import crypto from 'crypto';
import fs from 'fs';
import os from 'os';
import path from 'path';

const DEFAULT_PORT = '3001';

const HELP_MESSAGE = `\
usage: isl [--port PORT]

optional arguments:
  -h, --help       Show this message
  -f, --foreground Run the server process in the foreground.
  --no-open        Do not try to open a browser after starting the server
  -p, --port       Port to listen on (default: ${DEFAULT_PORT})
  --json           Output machine-readable JSON
  --stdout         Write server logs to stdout instead of a tmp file
  --dev            Open on port 3000 despite hosting ${DEFAULT_PORT} (or custom port with -p)
                   This is useless unless running from source to hook into CRA dev mode
  --kill           Do not start Sapling Web, just kill any previously running Sapling Web server on the specified port
                   Note that this will disrupt other windows still using the previous Sapling Web server.
  --force          Kill any existing Sapling Web server on the specified port, then start a new server.
                   Note that this will disrupt other windows still using the previous Sapling Web server.
  --command name   Set which command to run for sl commands (default: sl)
  --sl-version v   Set version number of sl was used to spawn the server (default: '(dev)')
  --platform       Set which platform implementation to use by changing the resulting URL.
                   Used to embed Sapling Web into non-browser web environments like IDEs.
`;

type JsonOutput =
  | {
      port: number;
      url: string;
      token: string;
      /** Process ID for the server. */
      pid: number;
      wasServerReused: boolean;
      logFileLocation: string | 'stdout';
      cwd: string;
      command: string;
    }
  | {error: string};

function errorAndExit(message: string, code = 1): never {
  // eslint-disable-next-line no-console
  console.error(message);
  process.exit(code);
}

type Args = {
  help: boolean;
  foreground: boolean;
  openUrl: boolean;
  port: number;
  isDevMode: boolean;
  json: boolean;
  stdout: boolean;
  platform: string | undefined;
  kill: boolean;
  force: boolean;
  slVersion: string;
  command: string;
};

// Rudimentary arg parser to avoid the need for a third-party dependency.
export function parseArgs(args: Array<string> = process.argv.slice(2)): Args {
  let help = false;
  // Before we added arg parsing, the $PORT environment variable was the only
  // way to set the port. Once callers have been updated to use --port, drop
  // support for the environment variable.
  let port = normalizePort(process.env.PORT || DEFAULT_PORT);
  let openUrl = true;

  const len = args.length;
  let isDevMode = false;
  let json = false;
  let stdout = false;
  let foreground = false;
  let kill = false;
  let force = false;
  let command = 'sl';
  let slVersion = '(dev)';
  let platform: string | undefined = undefined;
  let i = 0;
  function consumeArgValue(arg: string) {
    if (i >= len) {
      errorAndExit(`no value supplied for ${arg}`);
    } else {
      return args[++i];
    }
  }
  while (i < len) {
    const arg = args[i];
    switch (arg) {
      case '--no-open': {
        openUrl = false;
        break;
      }
      case '--foreground':
      case '-f': {
        foreground = true;
        break;
      }
      case '--port':
      case '-p': {
        const rawPort = consumeArgValue(arg);
        const parsedPort = normalizePort(rawPort);
        if (parsedPort !== false) {
          port = parsedPort as number;
        } else {
          errorAndExit(`could not parse port: '${rawPort}'`);
        }
        break;
      }
      case '--dev': {
        isDevMode = true;
        break;
      }
      case '--kill': {
        kill = true;
        break;
      }
      case '--force': {
        force = true;
        break;
      }
      case '--command': {
        command = consumeArgValue(arg);
        break;
      }
      case '--sl-version': {
        slVersion = consumeArgValue(arg);
        break;
      }
      case '--json': {
        json = true;
        break;
      }
      case '--stdout': {
        stdout = true;
        break;
      }
      case '--platform': {
        platform = consumeArgValue(arg);
        if (!isValidCustomPlatform(platform)) {
          errorAndExit(
            `"${platform}" is not a valid platform. Valid options: ${validPlatforms.join(', ')}`,
          );
        }
        break;
      }
      case '--help':
      case '-h': {
        help = true;
        break;
      }
      default: {
        errorAndExit(`unexpected arg: ${arg}`);
      }
    }
    ++i;
  }

  if (port === false) {
    errorAndExit('port was not a positive integer');
  }

  if (stdout && !foreground) {
    // eslint-disable-next-line no-console
    console.info('NOTE: setting --foreground because --stdout was specified');
    foreground = true;
  }

  if (kill && force) {
    // eslint-disable-next-line no-console
    console.info('NOTE: setting --kill and --force is redundant');
  }

  return {
    help,
    foreground,
    openUrl,
    port,
    isDevMode,
    json,
    stdout,
    platform,
    kill,
    force,
    slVersion,
    command,
  };
}

const CRYPTO_KEY_LENGTH = 128;

/** Generates a 128-bit secure random token. */
function generateToken(): Promise<string> {
  // crypto.generateKey() was introduced in v15.0.0. For earlier versions of
  // Node, we can use crypto.createDiffieHellman().
  if (typeof crypto.generateKey === 'function') {
    const {generateKey} = crypto;
    return new Promise((res, rej) =>
      generateKey('hmac', {length: CRYPTO_KEY_LENGTH}, (err, key) =>
        err ? rej(err) : res(key.export().toString('hex')),
      ),
    );
  } else {
    return Promise.resolve(
      crypto.createDiffieHellman(CRYPTO_KEY_LENGTH).generateKeys().toString('hex'),
    );
  }
}

const validPlatforms: Array<PlatformName> = ['androidStudio'];
function isValidCustomPlatform(name: unknown): name is PlatformName {
  return validPlatforms.includes(name as PlatformName);
}

/**
 * This calls the `startServer()` function that launches the server for ISL,
 * though the mechanism is conditional on the `foreground` param:
 *
 * - If `foreground` is true, then `startServer()` will be called directly as
 *   part of this process and it will continue run in the foreground. The user
 *   can do ctrl+c to kill the server (or ctrl+z to suspend it), as they would
 *   for any other process.
 * - If `foreground` is false, then we will spawn a new process via
 *   `child_process.fork()` that runs `child.ts` in this folder. IPC is done via
 *   `child.on('message')` and `process.send()`, though once this process has
 *   confirmed that the server is up and running, it can exit while the child
 *   will continue to run in the background.
 */
function callStartServer(args: StartServerArgs): Promise<StartServerResult> {
  if (args.foreground) {
    return import('./server').then(({startServer}) => startServer(args));
  } else {
    return new Promise(resolve => {
      // We pass the args via an environment variable because StartServerArgs
      // contains sensitive information and users on the system can see the
      // command line arguments of other users' processes, but not the
      // environment variables of other users' processes.
      //
      // We could also consider streaming the input as newline-delimited JSON
      // via stdin, though the max length for an environment variable seems
      // large enough for our needs.
      const env = {
        ...process.env,
        ISL_SERVER_ARGS: JSON.stringify({...args, logInfo: null}),
      };
      const options = {
        env,
        detached: true,
        // Child process should not inherit fds from the parent process, or
        // else something like `node run-proxy.js --json | jq` will never
        // terminate because the child process will keep stdout from the
        // parent process open, so jq will continue to read from it.
        stdio: 'ignore' as IOType,
      };
      const pathToChildModule = path.join(path.dirname(__filename), 'child');
      const child = child_process.fork(pathToChildModule, [], options);
      child.on('message', (message: ChildProcessResponse) => {
        switch (message.type) {
          case 'result': {
            resolve(message.result);
            break;
          }
          case 'message': {
            args.logInfo(...message.args);
            break;
          }
        }
      });
    });
  }
}

export async function runProxyMain(args: Args) {
  const {
    help,
    foreground,
    openUrl,
    port,
    isDevMode,
    json,
    stdout,
    platform,
    kill,
    force,
    slVersion,
    command,
  } = args;
  if (help) {
    errorAndExit(HELP_MESSAGE, 0);
  }

  const cwd = process.cwd();

  function info(...args: Parameters<typeof console.log>): void {
    if (json) {
      return;
    }
    // eslint-disable-next-line no-console
    console.info(...args);
  }

  /**
   * Output JSON information for use with `--json`.
   * Should only be called once per lifecycle of the server.
   */
  function outputJson(data: JsonOutput) {
    if (!json) {
      return;
    }
    // eslint-disable-next-line no-console
    console.log(JSON.stringify(data));
  }

  /////////////////////////////

  if (force) {
    // like kill, but don't exit the process, so we go on to start a fresh server
    let foundPid;
    try {
      foundPid = await killServerIfItExists(port, info);
      info(`killed Sapling Web server process ${foundPid}`);
    } catch (err: unknown) {
      info(`did not stop previous Sapling Web server: ${(err as Error).toString()}`);
    }
  } else if (kill) {
    let foundPid;
    try {
      foundPid = await killServerIfItExists(port, info);
    } catch (err: unknown) {
      errorAndExit((err as Error).toString());
    }
    info(`killed Sapling Web server process ${foundPid}`);
    process.exit(0);
  }

  // Since our spawned server can run processes and make authenticated requests,
  // we require a token in requests to match the one created here.
  // The sensitive token is given the to the client to authenticate requests.
  // The challenge token can be queried by the client to authenticate the server.
  const [sensitiveToken, challengeToken] = await Promise.all([generateToken(), generateToken()]);

  const logFileLocation = stdout
    ? 'stdout'
    : path.join(
        await fs.promises.mkdtemp(path.join(os.tmpdir(), 'isl-server-log')),
        'isl-server.log',
      );

  /**
   * Returns the URL the user can use to open ISL. Because this is often handed
   * off as an argument to another process, we must take great care when
   * constructing this argument.
   */
  function getURL(port: number, token: string, cwd: string): URL {
    // Although `port` is where our server is actually hosting from,
    // in dev mode CRA will start on 3000 and proxy requests to the server.
    // We only get the source build by opening from port 3000.
    const CRA_DEFAULT_PORT = 3000;

    let serverPort: number;
    if (isDevMode) {
      serverPort = CRA_DEFAULT_PORT;
    } else {
      if (!Number.isInteger(port) || port < 0) {
        throw Error(`illegal port: \`${port}\``);
      }
      serverPort = port;
    }
    const urlArgs: Record<string, string> = {
      token: encodeURIComponent(token),
      cwd: encodeURIComponent(cwd),
    };
    const platformPath =
      platform && platform !== 'browser' && isValidCustomPlatform(platform)
        ? `${encodeURIComponent(platform)}.html`
        : '';
    const url = `http://localhost:${serverPort}/${platformPath}?${Object.entries(urlArgs)
      .map(([key, value]) => `${key}=${value}`)
      .join('&')}`;
    return new URL(url);
  }

  /////////////////////////////

  const result = await callStartServer({
    foreground,
    port,
    sensitiveToken,
    challengeToken,
    logFileLocation,
    logInfo: info,
    command,
  });

  if (result.type === 'addressInUse' && !force) {
    // This port is already in use. Determine if it's a pre-existing ISL server,
    // and find the appropriate saved token, and reconstruct URL if recovered.

    const existingServerInfo = await lifecycle.readExistingServerFileWithRetries(port);
    if (!existingServerInfo) {
      const errorMessage =
        'failed to find existing server file. This port might not be being used by a Sapling Web server.\n' +
        suggestDebugPortIssue(port);
      if (json) {
        outputJson({
          error: errorMessage,
        });
      } else {
        info(errorMessage);
      }
      process.exit(1);
    }

    const pid = await lifecycle.checkIfServerIsAliveAndIsISL(info, port, existingServerInfo);
    if (pid == null) {
      const errorMessage =
        `port ${port} is already in use, but not by an Sapling Web server.\n` +
        suggestDebugPortIssue(port);
      if (json) {
        outputJson({
          error: errorMessage,
        });
      } else {
        info(errorMessage);
      }
      process.exit(1);
    }

    let killAndSpawnAgain = false;
    if (existingServerInfo.command !== command) {
      info(
        `warning: Starting a fresh server to use command '${command}' (existing server was using '${existingServerInfo.command}').`,
      );
      killAndSpawnAgain = true;
    } else if (existingServerInfo.slVersion !== slVersion) {
      info(
        `warning: sl version has changed since last server was started. Starting a fresh server to use lastest version '${slVersion}'.`,
      );
      killAndSpawnAgain = true;
    }

    if (killAndSpawnAgain) {
      try {
        await killServerIfItExists(port, info);
      } catch (err: unknown) {
        errorAndExit(`did not stop previous Sapling Web server: ${(err as Error).toString()}`);
      }

      // Now that we killed the server, try the whole thing again to spawn a new instance.
      // We're guaranteed to not go down the same code path since the last authentic server was killed.
      // We also know --force or --kill could not have been supplied.
      await runProxyMain(args);
      return;
    }

    const url = getURL(port as number, existingServerInfo.sensitiveToken, cwd);
    info('re-used existing Sapling Web server');
    info('\naccess Sapling Web with this link:');
    info(String(url));

    if (json) {
      outputJson({
        url: url.href,
        port: port as number,
        token: existingServerInfo.sensitiveToken,
        pid,
        wasServerReused: true,
        logFileLocation: existingServerInfo.logFileLocation,
        cwd,
        command: existingServerInfo.command,
      });
    } else if (openUrl) {
      maybeOpenURL(url);
    }
    process.exit(0);
  } else if (result.type === 'success') {
    // The server successfully started on this port
    // Save the server information for re-use and print the URL for use

    try {
      await ensureExistingServerFolder();
      await deleteExistingServerFile(port);
      await writeExistingServerFile(port, {
        sensitiveToken,
        challengeToken,
        logFileLocation,
        command,
        slVersion,
      });
    } catch (error) {
      info(
        'failed to save server information re-use. ' +
          'This server will remain, but future invocations on this port will not be able to re-use this instance.',
        error,
      );
    }

    const {port: portInUse} = result;
    const url = getURL(portInUse, sensitiveToken, cwd);
    info('started a new server');
    info('\naccess Sapling Web with this link:');
    info(String(url));
    if (json) {
      outputJson({
        url: url.href,
        port: portInUse,
        token: sensitiveToken,
        pid: result.pid,
        wasServerReused: false,
        logFileLocation,
        cwd,
        command,
      });
    }

    if (openUrl) {
      maybeOpenURL(url);
    }

    // If --foreground was not specified, we can kill this process, but the
    // web server in the child process will stay alive.
    if (!foreground) {
      process.exit(0);
    }
  } else if (result.type === 'error') {
    errorAndExit(result.error);
  }
}

/**
 * Finds any existing ISL server process running on `port`.
 * If one is found, it is killed and the PID is returned.
 * Otherwise, an error is returned
 */
export async function killServerIfItExists(
  port: number,
  info: typeof console.info,
): Promise<number> {
  const existingServerInfo = await lifecycle.readExistingServerFileWithRetries(port);
  if (!existingServerInfo) {
    throw new Error(`could not find existing server information to kill on port ${port}`);
  }
  const pid = await lifecycle.checkIfServerIsAliveAndIsISL(
    info,
    port,
    existingServerInfo,
    /* silent */ true,
  );
  if (!pid) {
    throw new Error(`could not find existing server process to kill on port ${port}`);
  }
  try {
    process.kill(pid);
  } catch (err) {
    throw new Error(
      `could not kill previous Sapling Web server process with PID ${pid}. This instance may no longer be running.`,
    );
  }
  return pid;
}

/**
 * Normalize a port into a number or false.
 */
function normalizePort(val: string): number | false {
  const port = parseInt(val, 10);
  return !isNaN(port) && port >= 0 ? port : false;
}

/**
 * Text to include in an error message to help the user self-diagnose their
 * "port already in use" issue.
 */
function suggestDebugPortIssue(port: number): string {
  if (process.platform !== 'win32') {
    return (
      `try running \`lsof -i :${port}\` to see what is running on port ${port}, ` +
      'or just try using a different port.'
    );
  } else {
    return 'try using a different port.';
  }
}

/**
 * Because `url` will be passed to the "opener" executable for the platform,
 * the caller must take responsibility for ensuring the integrity of the
 * `url` argument.
 */
function maybeOpenURL(url: URL): void {
  const {href} = url;
  // Basic sanity checking: this does not eliminate all illegal inputs.
  if (!href.startsWith('http://') || href.indexOf(' ') !== -1) {
    throw Error(`illegal URL: \`href\``);
  }

  let openCommand: string;
  let commandOptions: string[] | null = null;
  switch (process.platform) {
    case 'darwin': {
      openCommand = '/usr/bin/open';
      break;
    }
    case 'win32': {
      // We cannot use `powershell -command 'start <URL>'` because then
      // `start <URL>` is a single argument and we have to worry about
      // escaping it safely. We use this construction in combination with
      // `windowsVerbatimArguments: true` below so that we do not have
      // to take responsibility for escaping the URL.
      openCommand = 'cmd';
      commandOptions = ['/c', 'start'];
      break;
    }
    default: {
      openCommand = 'xdg-open';
      break;
    }
  }

  const args = commandOptions != null ? commandOptions.concat(href) : [href];

  // Note that if openCommand does not exist on the host, this will fail with
  // ENOENT. Often, this is fine: the user could start isl on a headless
  // machine, but then set up tunneling to reach the server from another host.
  const child = child_process.spawn(openCommand, args, {
    detached: true,
    stdio: 'ignore' as IOType,
    windowsHide: true,
    windowsVerbatimArguments: true,
  });

  // While `/usr/bin/open` on macOS and `start` on Windows are expected to be
  // available, xdg-open is not guaranteed, so report an appropriate error in
  // this case.
  child.on('error', (error: NodeJS.ErrnoException) => {
    if (error.code === 'ENOENT') {
      // eslint-disable-next-line no-console
      console.error(
        `command \`${openCommand}\` not found: run with --no-open to suppress this message`,
      );
    } else {
      // eslint-disable-next-line no-console
      console.error(
        `unexpected error running command \`${openCommand} ${args.join(' ')}\`:`,
        error,
      );
    }
  });
}
