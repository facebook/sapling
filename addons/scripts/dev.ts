/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {EjecaChildProcess} from 'shared/ejeca';

import chalk from 'chalk';
import {ejeca} from 'shared/ejeca';

function usage() {
  process.stdout.write(`
${chalk.bold('yarn dev')} - Combined server + client builds for ISL.

${chalk.bold('Usage:')} yarn dev [browser|vscode]
  --production   Build in production mode

${chalk.bold('Examples:')}
  yarn dev browser
    ${chalk.gray('Build client and server in development mode, watch for changes')}
  yarn dev browser --production
    ${chalk.gray('Build client and server in production mode, without watching for changes')}

  yarn dev vscode
    ${chalk.gray('Build extension and webview in dev mode, watch for changes')}
  yarn dev vscode --production
    ${chalk.gray('Build extension and webview in production mode, without watching for changes')}
`);
}

type Args = {
  kind: 'browser' | 'vscode';
  isProduction: boolean;
};
function parseArgs(): Args {
  const args = process.argv.slice(2);
  if (args.includes('--help')) {
    usage();
    process.exit(0);
  }
  const kind = args[0] as 'browser' | 'vscode';
  if (!['browser', 'vscode'].includes(kind)) {
    process.stdout.write(
      kind ? chalk.red('Unknown kind:', kind) : chalk.red('Missing kind') + '\n',
    );
    usage();
    process.exit(1);
  }

  const isProduction = args.includes('--production');
  // vite/rollup look for this env var
  process.env.NODE_ENV = isProduction ? 'production' : 'development';

  return {
    kind,
    isProduction,
  };
}

const MOVE_TO_START = '\x1b[0G\x1b[K';
const CLEAR_LINE = '\x1b[K';
const MOVE_UP_1 = '\x1b[1A';

/**
 * Spawn multiple processes, show their execution status and output in parallel
 * While processes run, only a few lines of output are shown, and long lines are truncated.
 * At the end, all output is shown again, without truncation.
 *
 * Output looks like this:
      isl/ $ yarn start
      ┃ VITE v5.4.7  ready in 1132 ms
      ┃
      ┃ ➜  Local:   http://localhost:3000/
      ┃ ➜  Network: use --host to expose
      ┃ ➜  press h + enter to show help
      ┗━ Running...

      isl-server/ $ yarn watch
      ┃ created dist in 772ms
      ┃
      ┃ [2025-01-24 11:26:05] waiting for changes...
      ┗━ Running...
 */
class MultiRunner {
  private infos: Array<{
    handle?: EjecaChildProcess;
    output: Array<string>;
    start?: Date;
    end?: Date;
    status?: string;
  }>;
  constructor(
    public configs: Array<{
      cwd: string;
      cmd: string;
      args: Array<string>;
      /** Provide this callback to change the status label.
       * It gets called for each new chunk of output, and the status persists until it is changed again.
       * For example, detect that the build command is ready, and change "Running..." to "Ready, watching for changes..."  */
      customStatus?: (chunk: string, status?: string) => string | undefined;
    }>,
  ) {
    this.infos = configs.map(() => {
      return {
        handle: undefined,
        output: [],
        start: undefined,
        end: undefined,
        status: undefined,
      };
    });
  }

  async spawnAll() {
    this.configs.forEach(({cwd, cmd, args}, i) => {
      const info = this.infos[i]!;
      info.start = new Date();
      const proc = ejeca(cmd, args, {cwd, stdout: 'pipe', stderr: 'pipe'});
      const output: Array<string> = [];
      proc.stdout!.on('data', data => {
        output.push(...data.toString().split('\n'));
        this.updateCustomStatus(i, data.toString());
        this.redraw();
      });
      proc.stderr!.on('data', data => {
        output.push(...data.toString().split('\n'));
        this.updateCustomStatus(i, data.toString());
        this.redraw();
      });
      proc.stderr!.on('close', () => {
        this.infos[i]!.end = new Date();
        this.redraw();
      });
      info.handle = proc;
      info.output = output;
    });
    this.redraw();
    await Promise.all(this.infos.map(info => info.handle));

    // Redraw one last time, with all output
    this.redraw(/* printAllOutput */ true);
  }

  private updateCustomStatus(i: number, chunk: string) {
    const customStatus = this.configs[i].customStatus;
    if (customStatus) {
      const status = this.infos[i].status;
      this.infos[i].status = customStatus(chunk, status);
    }
    return undefined;
  }

  private lastNumLines = 0;
  redraw(printAllOutput = false) {
    if (this.lastNumLines > 0) {
      process.stdout.write(MOVE_TO_START + CLEAR_LINE);
      process.stdout.write((MOVE_UP_1 + CLEAR_LINE).repeat(this.lastNumLines)); // move cursor up and clear
    }

    let totalLines = 0;
    for (let i = 0; i < this.infos.length; i++) {
      const {cwd, cmd, args} = this.configs[i];
      const {handle, output: lines, start, end, status} = this.infos[i];

      const durationMs = end == null || start == null ? null : end.valueOf() - start.valueOf();
      const durationStr =
        durationMs == null
          ? ''
          : chalk.gray(
              ` in ${
                durationMs > 1_000
                  ? (durationMs / 1000).toLocaleString() + ' s'
                  : durationMs.toLocaleString() + ' ms'
              }`,
            );
      const statusStr =
        handle == null
          ? chalk.gray('Waiting...')
          : handle.exitCode == null
          ? status ?? chalk.gray('Running...')
          : handle.exitCode === 0
          ? chalk.green(`Suceeded${durationStr}`)
          : chalk.red(`Exited ${handle.exitCode}`);

      const output = [];

      output.push(
        `${chalk.bold.cyan(cwd + '/')} ${chalk.yellowBright('$')} ${cmd} ${args.join(' ')}`,
      );
      const LINES_TO_SHOW = printAllOutput
        ? Infinity
        : process.stdout.columns / this.infos.length - 5; // Fit all processes on the screen, with padding
      if (lines.length > LINES_TO_SHOW + 1) {
        output.push(
          `${chalk.cyan('┣━ ')} ${chalk.gray(
            `...${lines.length - LINES_TO_SHOW} lines hidden...`,
          )}`,
        );
      }
      for (const line of lines.slice(-LINES_TO_SHOW)) {
        const ELLIPSIS = '…';
        const maxLen = process.stdout.columns - 4;
        const truncated = printAllOutput
          ? line
          : line.length > maxLen
          ? line.slice(0, maxLen) + ELLIPSIS
          : line;
        output.push(`${chalk.cyan('┃ ')}${truncated}`);
      }
      output.push(`${chalk.cyan('┗━ ')} ${chalk.gray(statusStr)}`);

      process.stdout.write(output.join('\n') + `\n\n`);
      totalLines += output.length + 1;
    }

    this.lastNumLines = totalLines;
  }
}

async function main() {
  const args = parseArgs();
  const {kind, isProduction} = args;
  const runner = new MultiRunner(
    kind === 'vscode'
      ? [
          {
            cwd: 'vscode',
            cmd: 'yarn',
            args: isProduction ? ['build-webview'] : ['watch-webview'],
            customStatus: isProduction
              ? undefined
              : (chunk: string, status?: string) => {
                  if (chunk.includes('ready in')) {
                    return chalk.green('Webview Ready') + ' watching for changes...';
                  }
                  return status;
                },
          },
          {
            cwd: 'vscode',
            cmd: 'yarn',
            args: isProduction ? ['build-extension'] : ['watch-extension'],
            customStatus: isProduction
              ? undefined
              : (chunk: string, status?: string) => {
                  if (chunk.includes('created ')) {
                    return chalk.green('Extension Ready') + ' watching for changes...';
                  }
                  return status;
                },
          },
        ]
      : [
          {
            cwd: 'isl',
            cmd: 'yarn',
            args: isProduction ? ['build'] : ['start'],
            customStatus: isProduction
              ? undefined
              : (chunk: string, status?: string) => {
                  if (chunk.includes('ready in')) {
                    return chalk.green('Client Ready') + ' watching for changes...';
                  }
                  return status;
                },
          },
          {
            cwd: 'isl-server',
            cmd: 'yarn',
            args: isProduction ? ['build'] : ['watch'],
            customStatus: isProduction
              ? undefined
              : (chunk: string, status?: string) => {
                  if (chunk.includes('created ')) {
                    return chalk.green('Server Ready') + ' watching for changes...';
                  }
                  return status;
                },
          },
        ],
  );

  await runner.spawnAll();
}

main();
