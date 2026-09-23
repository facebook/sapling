/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import WebSocket from 'ws';
import {startWebSocketKeepAlive, WEBSOCKET_KEEPALIVE_INTERVAL_MS} from '../proxyUtils';

type PingCallback = (error?: Error) => void;
type ReadyState = WebSocket['readyState'];
type FakeSocket = {readyState: ReadyState; ping: jest.Mock};

function fakeSocket(readyState: ReadyState, ping: jest.Mock = jest.fn()): FakeSocket {
  return {readyState, ping};
}

/** The callback the keepalive handed to the nth ping. */
function pingCallback(ping: jest.Mock, nth = 0): PingCallback {
  const callback = ping.mock.calls[nth][2];
  if (typeof callback !== 'function') {
    throw new Error('keepalive did not pass a completion callback to ping');
  }
  return callback as PingCallback;
}

describe('startWebSocketKeepAlive', () => {
  beforeEach(() => {
    jest.useFakeTimers();
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it('pings an open socket once per interval until stopped', () => {
    const socket = fakeSocket(WebSocket.OPEN);
    const stop = startWebSocketKeepAlive(socket);

    jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS - 1);
    expect(socket.ping).not.toHaveBeenCalled();
    jest.advanceTimersByTime(1);
    expect(socket.ping).toHaveBeenCalledTimes(1);
    jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS * 2);
    expect(socket.ping).toHaveBeenCalledTimes(3);

    stop();
    jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS * 2);
    expect(socket.ping).toHaveBeenCalledTimes(3);
  });

  it('skips a socket that is no longer open', () => {
    const socket = fakeSocket(WebSocket.CLOSING);
    const stop = startWebSocketKeepAlive(socket);

    jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS * 2);
    expect(socket.ping).not.toHaveBeenCalled();

    stop();
  });

  it('resumes pinging when a socket that was not open becomes open', () => {
    const socket = fakeSocket(WebSocket.CONNECTING);
    const stop = startWebSocketKeepAlive(socket);

    jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS);
    expect(socket.ping).not.toHaveBeenCalled();

    socket.readyState = WebSocket.OPEN;
    jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS);
    expect(socket.ping).toHaveBeenCalledTimes(1);

    stop();
  });

  it('reports a write failure through onError and keeps pinging', () => {
    const socket = fakeSocket(WebSocket.OPEN);
    const onError = jest.fn();
    const stop = startWebSocketKeepAlive(socket, undefined, onError);

    jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS);
    const failure = new Error('EPIPE');
    pingCallback(socket.ping).call(undefined, failure);
    expect(onError).toHaveBeenCalledWith(failure);

    // A successful write reports nothing.
    jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS);
    pingCallback(socket.ping, 1).call(undefined);
    expect(onError).toHaveBeenCalledTimes(1);
    expect(socket.ping).toHaveBeenCalledTimes(2);

    stop();
  });

  it('survives a ping that throws and keeps pinging', () => {
    const ping = jest.fn(() => {
      throw new Error('WebSocket is not open: readyState 0 (CONNECTING)');
    });
    const socket = fakeSocket(WebSocket.OPEN, ping);
    const onError = jest.fn();
    const stop = startWebSocketKeepAlive(socket, undefined, onError);

    expect(() => jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS * 3)).not.toThrow();
    expect(ping).toHaveBeenCalledTimes(3);
    expect(onError).toHaveBeenCalledTimes(3);
    expect(onError.mock.calls[0][0]).toBeInstanceOf(Error);

    stop();
  });

  it('wraps a non-Error throw and tolerates a throwing reporter', () => {
    const ping = jest.fn(() => {
      // eslint-disable-next-line no-throw-literal
      throw 'boom';
    });
    const socket = fakeSocket(WebSocket.OPEN, ping);
    const onError = jest.fn(() => {
      throw new Error('reporter exploded');
    });
    const stop = startWebSocketKeepAlive(socket, undefined, onError);

    expect(() => jest.advanceTimersByTime(WEBSOCKET_KEEPALIVE_INTERVAL_MS * 2)).not.toThrow();
    expect(onError).toHaveBeenCalledTimes(2);
    expect((onError.mock.calls[0] as unknown[])[0]).toEqual(new Error('boom'));

    stop();
  });

  it('does not hold the process open on its own', () => {
    const unref = jest.fn();
    const setIntervalSpy = jest
      .spyOn(global, 'setInterval')
      .mockReturnValue({unref} as unknown as NodeJS.Timeout);
    try {
      const stop = startWebSocketKeepAlive(fakeSocket(WebSocket.OPEN));
      expect(unref).toHaveBeenCalledTimes(1);
      stop();
    } finally {
      setIntervalSpy.mockRestore();
    }
  });

  it('honours a custom interval', () => {
    const socket = fakeSocket(WebSocket.OPEN);
    const stop = startWebSocketKeepAlive(socket, 1_000);

    jest.advanceTimersByTime(999);
    expect(socket.ping).not.toHaveBeenCalled();
    jest.advanceTimersByTime(1);
    expect(socket.ping).toHaveBeenCalledTimes(1);

    stop();
  });
});
