/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {CancellationToken} from '../CancellationToken';

describe('CancellationToken', () => {
  it('can be cancelled', () => {
    const token = new CancellationToken();
    expect(token.isCancelled).toBe(false);
    token.cancel();
    expect(token.isCancelled).toBe(true);
  });

  it('can be subscribed to', () => {
    const token = new CancellationToken();
    const callback = jest.fn();
    const dispose = token.onCancel(callback);

    token.cancel();

    expect(callback).toHaveBeenCalled();

    dispose();
  });

  it('callback can be disposed', () => {
    const token = new CancellationToken();
    const callback = jest.fn();
    const dispose = token.onCancel(callback);
    dispose();
    token.cancel();
    expect(callback).not.toHaveBeenCalled();
  });

  it('handles multiple callbacks', () => {
    const token = new CancellationToken();
    const callback1 = jest.fn();
    const callback2 = jest.fn();
    const callback3 = jest.fn();
    const dispose1 = token.onCancel(callback1);
    const dispose2 = token.onCancel(callback2);
    const dispose3 = token.onCancel(callback3);

    dispose2();

    token.cancel();
    expect(callback1).toHaveBeenCalledTimes(1);
    expect(callback2).not.toHaveBeenCalled();
    expect(callback3).toHaveBeenCalledTimes(1);

    dispose1();
    dispose3();
  });

  it('callback fires immediately when already cancelled', () => {
    const token = new CancellationToken();
    token.cancel();
    const callback = jest.fn();
    const dispose = token.onCancel(callback);
    expect(callback).toHaveBeenCalled();

    dispose();
  });

  it('cancelling is idempotent', () => {
    const token = new CancellationToken();
    const callback = jest.fn();
    const dispose = token.onCancel(callback);

    token.cancel();
    token.cancel();
    token.cancel();

    expect(callback).toHaveBeenCalledTimes(1);

    dispose();
  });
});
