/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {TypedEventEmitter} from '../TypedEventEmitter';

describe('TypedEventEmitter', () => {
  it('can emit and listen to events', () => {
    const emitter = new TypedEventEmitter<'foo', number>();
    const onEmit = jest.fn();
    emitter.on('foo', onEmit);
    emitter.emit('foo', 1);
    expect(onEmit).toHaveBeenCalledWith(1);
    emitter.emit('foo', 2);
    expect(onEmit).toHaveBeenCalledWith(2);

    emitter.off('foo', onEmit);
    emitter.emit('foo', 3);
    expect(onEmit).not.toHaveBeenCalledWith(3);
  });

  it('always allows emitting errors', () => {
    const emitter = new TypedEventEmitter<'foo', number>();
    const onEmit = jest.fn();
    const onError = jest.fn();

    emitter.on('foo', onEmit);
    emitter.on('error', onError);

    emitter.emit('foo', 1);
    expect(onEmit).toHaveBeenCalledWith(1);
    emitter.emit('error', new Error('uh oh'));
    expect(onError).toHaveBeenCalledWith(new Error('uh oh'));

    emitter.off('error', onError);
    emitter.emit('error', new Error('not listened to'));
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it('removeAllListeners', () => {
    const emitter = new TypedEventEmitter<'foo', number>();
    const onEmit = jest.fn();
    const onError = jest.fn();

    emitter.on('foo', onEmit);
    emitter.on('error', onError);

    emitter.emit('error', new Error('blah'));
    emitter.emit('foo', 3);

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onEmit).toHaveBeenCalledTimes(1);

    emitter.removeAllListeners();

    emitter.emit('error', new Error('blah'));
    emitter.emit('foo', 3);

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onEmit).toHaveBeenCalledTimes(1);
  });
});
