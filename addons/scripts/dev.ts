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
  private handles = Array<EjecaChildProcess>();
  private output = Array<Array<string>>();
  private timing = Array<{start: Date; end?: Date}>();
  constructor(public configs: Array<{cwd: string; cmd: string; args: Array<string>}>) {}

  async spawnAll() {
    this.handles = this.configs.map(({cwd, cmd, args}, i) => {
      const proc = ejeca(cmd, args, {cwd, stdout: 'pipe', stderr: 'pipe'});
      this.output[i] = [];
      this.timing[i] = {start: new Date(), end: undefined};
      proc.stdout!.on('data', data => {
        this.output[i].push(...data.toString().split('\n'));
        this.redraw();
      });
      proc.stderr!.on('data', data => {
        this.output[i].push(...data.toString().split('\n'));
        this.redraw();
      });
      proc.stderr!.on('close', () => {
        this.timing[i].end = new Date();
        this.redraw();
      });
      return proc;
    });
    this.redraw();
    await Promise.all(this.handles);

    // Redraw one last time, with all output
    this.redraw(/* printAllOutput */ true);
  }

  private lastNumLines = 0;
  redraw(printAllOutput = false) {
    if (this.lastNumLines > 0) {
      process.stdout.write(MOVE_TO_START + CLEAR_LINE);
      process.stdout.write((MOVE_UP_1 + CLEAR_LINE).repeat(this.lastNumLines)); // move cursor up and clear
    }

    let totalLines = 0;
    for (let i = 0; i < this.handles.length; i++) {
      const {cwd, cmd, args} = this.configs[i];
      const proc = this.handles[i];
      const lines = this.output[i];
      const timing = this.timing[i];

      const durationMs =
        timing.end == null ? null : timing.end.valueOf() - this.timing[i].start.valueOf();
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
        proc.exitCode == null
          ? chalk.gray('Running...')
          : proc.exitCode === 0
          ? chalk.green(`Suceeded${durationStr}`)
          : chalk.red(`Exited ${proc.exitCode}`);

      const output = [];

      output.push(
        `${chalk.bold.cyan(cwd + '/')} ${chalk.yellowBright('$')} ${cmd} ${args.join(' ')}`,
      );
      const LINES_TO_SHOW = printAllOutput
        ? Infinity
        : process.stdout.columns / this.handles.length - 5; // Fit all processes on the screen, with padding
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
          {cwd: 'vscode', cmd: 'yarn', args: isProduction ? ['build-webview'] : ['watch-webview']},
          {
            cwd: 'vscode',
            cmd: 'yarn',
            args: isProduction ? ['build-extension'] : ['watch-extension'],
          },
        ]
      : [
          {cwd: 'isl', cmd: 'yarn', args: isProduction ? ['build'] : ['start']},
          {cwd: 'isl-server', cmd: 'yarn', args: isProduction ? ['build'] : ['watch']},
        ],
  );

  await runner.spawnAll();
}

main();
