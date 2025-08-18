/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/* eslint-disable no-console */

import serverAPI from '../ClientToServerAPI';

export async function runCodeReview(cwd: string): Promise<void> {
  // Ensure devmate is installed
  serverAPI.postMessage({type: 'runDevmateCommand', args: ['--help'], cwd});
  const devmateInstallationStatus = (
    await serverAPI.nextMessageMatching('devmateCommandResult', () => true)
  ).result;
  if (devmateInstallationStatus.type === 'error') {
    // Devmate is not available, so we can't run code review
    console.log('Devmate is not available.');
    console.log(devmateInstallationStatus.stderr);
    return;
  }

  // Run code review
  serverAPI.postMessage({
    type: 'runDevmateCommand',
    args: ['run', 'mcp_servers/code_review/review_code.md'],
    cwd,
  });
  const codeReviewResult = (await serverAPI.nextMessageMatching('devmateCommandResult', () => true))
    .result;
  if (codeReviewResult.type === 'error') {
    // Devmate failed to run code review
    console.log('Devmate failed to run code review.');
    console.log(codeReviewResult.stderr);
    return;
  }

  console.log('Code review completed successfully!');
  console.log(codeReviewResult.stdout);
}
