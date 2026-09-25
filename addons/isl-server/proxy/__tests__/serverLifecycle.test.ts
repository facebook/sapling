/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import http from 'node:http';
import {checkIfServerIsAliveAndIsISL} from '../serverLifecycle';

it('omits credential-bearing errors from server authentication diagnostics', async () => {
  const request = jest.spyOn(http, 'request').mockImplementation(() => {
    throw new Error('request failed: /challenge_authenticity?token=test-capability');
  });
  const info = jest.fn();
  try {
    expect(
      await checkIfServerIsAliveAndIsISL(info, 3011, {
        sensitiveToken: 'test-capability',
        challengeToken: 'test-challenge',
        logFileLocation: '/tmp/isl-test.log',
        command: 'sl',
        slVersion: 'test',
      }),
    ).toBeNull();
    expect(info).toHaveBeenCalledWith(
      'error checking if existing Sapling Web server on port 3011 is authentic',
    );
  } finally {
    request.mockRestore();
  }
});
