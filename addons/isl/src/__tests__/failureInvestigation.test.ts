/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {createFailedOperationContext, parseFailedOperationContext} from '../failureInvestigation';

describe('failure investigation evidence', () => {
  it('retains the last 100 lines and identifies truncation', () => {
    const context = createFailedOperationContext(
      'id',
      'GotoOperation',
      1,
      Array.from({length: 150}, (_, i) => String(i)),
    );
    expect(context.output.split('\n')).toEqual(Array.from({length: 100}, (_, i) => String(i + 50)));
    expect(context.outputTruncated).toBe(true);
    expect(parseFailedOperationContext(context)).toEqual(context);
  });

  it('bounds multibyte output without splitting a code point', () => {
    const context = createFailedOperationContext('id', 'GotoOperation', 1, ['😀'.repeat(10000)]);
    expect(new TextEncoder().encode(context.output).length).toBeLessThanOrEqual(16384);
    expect(context.output).not.toContain('\ufffd');
    expect(context.outputTruncated).toBe(true);
    expect(parseFailedOperationContext(context)).toEqual(context);
  });

  it('drops extra fields without interpreting error text', () => {
    const context = createFailedOperationContext('id', 'GotoOperation', 1, ['$(do-not-run)']);
    expect(
      parseFailedOperationContext({...context, args: ['danger'], workspace: '/other'}),
    ).toEqual(context);
  });
});
