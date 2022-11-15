/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/* eslint-disable @typescript-eslint/no-explicit-any */
/* eslint-disable @typescript-eslint/no-unused-vars */
/* eslint-disable @typescript-eslint/no-this-alias */

import type {Writable} from 'shared/typeUtils';

import {LocalWebSocketEventBus} from '../MessageBus';

jest.mock('../urlParams', () => ({
  initialParams: new Map([['token', '1234']]),
}));

let globalMockWs: MockWebSocketImpl;
class MockWebSocketImpl extends EventTarget implements WebSocket {
  constructor(public url: string, _protocols?: string | string[] | undefined) {
    super();
    globalMockWs = this as unknown as MockWebSocketImpl; // keep track of each new instance as a global to use in tests
  }

  binaryType = 'blob' as const;
  bufferedAmount = 0;
  extensions = '';

  onclose = null;
  onerror = null;
  onmessage = null;
  onopen = null;
  protocol = '';
  readyState = 0;

  readonly OPEN = 0 as const;
  readonly CONNECTING = 1 as const;
  readonly CLOSED = 2 as const;
  readonly CLOSING = 3 as const;

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void {
    this.sentMessages.push(data as string);
  }

  // eslint-disable-next-line @typescript-eslint/no-empty-function
  close(_code?: number, _reason?: string): void {}

  // -------- Additional APIs for testing --------

  simulateIncomingMessage(message: string) {
    const e = new Event('message');
    (e as Writable<MessageEvent<string>>).data = message;
    this.dispatchEvent(e);
  }
  simulateServerConnected() {
    this.dispatchEvent(new Event('open'));
  }
  simulateServerDisconnected() {
    this.dispatchEvent(new Event('close'));
  }

  public sentMessages: Array<string> = [];
}
const MockWebSocket = MockWebSocketImpl as unknown as typeof WebSocket;

const DEFAULT_HOST = 'localhost:8080';

function createMessageBus(): LocalWebSocketEventBus {
  return new LocalWebSocketEventBus(DEFAULT_HOST, MockWebSocket);
}

describe('LocalWebsocketEventBus', () => {
  it('opens and sends messages', () => {
    const bus = createMessageBus();
    globalMockWs.simulateServerConnected();
    bus.postMessage('my message');
    expect(globalMockWs.sentMessages).toEqual(['my message']);
  });

  it('queues messages while connecting', () => {
    const bus = createMessageBus();
    bus.postMessage('first');
    bus.postMessage('second');
    expect(globalMockWs.sentMessages).toEqual([]);
    globalMockWs.simulateServerConnected();
    bus.postMessage('third');
    expect(globalMockWs.sentMessages).toEqual(['first', 'second', 'third']);
  });

  it('handles incoming messages', () => {
    const bus = createMessageBus();
    const onMessage1 = jest.fn();
    const onMessage2 = jest.fn();
    bus.onMessage(onMessage1);
    bus.onMessage(onMessage2);

    globalMockWs.simulateServerConnected();
    globalMockWs.simulateIncomingMessage('incoming message');

    expect(onMessage1).toHaveBeenCalledWith(expect.objectContaining({data: 'incoming message'}));
    expect(onMessage2).toHaveBeenCalledWith(expect.objectContaining({data: 'incoming message'}));
  });

  it('notifies about status', () => {
    const bus = createMessageBus();
    const changeStatus = jest.fn();
    bus.onChangeStatus(changeStatus);

    expect(changeStatus).toHaveBeenCalledWith({type: 'initializing'});
    globalMockWs.simulateServerConnected();

    expect(changeStatus).toHaveBeenCalledWith({type: 'open'});
    expect(changeStatus).toHaveBeenCalledTimes(2);
    changeStatus.mockClear();

    globalMockWs.simulateServerDisconnected();
    expect(changeStatus).toHaveBeenCalledWith({type: 'reconnecting'});
    expect(changeStatus).not.toHaveBeenCalledWith({type: 'open'});

    globalMockWs.simulateServerConnected();
    expect(changeStatus).toHaveBeenCalledWith({type: 'open'});
  });

  it('disposes status handlers properly', () => {
    const bus = createMessageBus();

    const changeStatus1 = jest.fn();
    bus.onChangeStatus(changeStatus1);

    const changeStatus2 = jest.fn();
    const disposable2 = bus.onChangeStatus(changeStatus2);

    const changeStatus3 = jest.fn();
    bus.onChangeStatus(changeStatus3);

    expect(changeStatus1).toHaveBeenCalledWith({type: 'initializing'});
    expect(changeStatus2).toHaveBeenCalledWith({type: 'initializing'});
    expect(changeStatus3).toHaveBeenCalledWith({type: 'initializing'});

    disposable2.dispose();

    globalMockWs.simulateServerConnected();

    expect(changeStatus1).toHaveBeenCalledWith({type: 'open'});
    expect(changeStatus2).not.toHaveBeenCalledWith({type: 'open'});
    expect(changeStatus3).toHaveBeenCalledWith({type: 'open'});
  });

  it('queues up messages while disconnected', () => {
    const bus = createMessageBus();
    globalMockWs.simulateServerConnected();

    expect(globalMockWs.sentMessages).toEqual([]);

    globalMockWs.simulateServerDisconnected();

    bus.postMessage('hi');
    expect(globalMockWs.sentMessages).toEqual([]);
    globalMockWs.simulateServerConnected();
    expect(globalMockWs.sentMessages).toEqual(['hi']);
  });

  it('previous onMessage handlers exist after reconnection', () => {
    const bus = createMessageBus();

    const onMessage = jest.fn();
    bus.onMessage(onMessage);

    globalMockWs.simulateServerConnected();

    globalMockWs.simulateIncomingMessage('one');
    expect(onMessage).toHaveBeenCalledWith(expect.objectContaining({data: 'one'}));

    globalMockWs.simulateServerDisconnected();
    globalMockWs.simulateServerConnected();

    globalMockWs.simulateIncomingMessage('two');
    expect(onMessage).toHaveBeenCalledWith(expect.objectContaining({data: 'two'}));
  });

  it('clears queued messages after sending them', () => {
    const bus = createMessageBus();
    globalMockWs.simulateServerConnected();

    expect(globalMockWs.sentMessages).toEqual([]);

    globalMockWs.simulateServerDisconnected();

    bus.postMessage('hi');
    expect(globalMockWs.sentMessages).toEqual([]);
    globalMockWs.simulateServerConnected();
    expect(globalMockWs.sentMessages).toEqual(['hi']);

    globalMockWs.simulateServerDisconnected();
    globalMockWs.simulateServerConnected();

    expect(globalMockWs.sentMessages).toEqual(['hi']);
  });

  it('disposes handlers properly', () => {
    const bus = createMessageBus();
    globalMockWs.simulateServerConnected();

    const onMessage = jest.fn();
    const disposable = bus.onMessage(onMessage);

    globalMockWs.simulateServerConnected();
    globalMockWs.simulateIncomingMessage('incoming message');

    expect(onMessage).toHaveBeenCalledWith(expect.objectContaining({data: 'incoming message'}));
    disposable.dispose();
    globalMockWs.simulateIncomingMessage('another after dispose');
    expect(onMessage).not.toHaveBeenCalledWith(
      expect.objectContaining({data: 'another after dispose'}),
    );
  });

  it('disposes only one handler at a time', () => {
    const bus = createMessageBus();
    globalMockWs.simulateServerConnected();

    const onMessage1 = jest.fn();
    bus.onMessage(onMessage1);

    const onMessage2 = jest.fn();
    const disposable2 = bus.onMessage(onMessage2);

    const onMessage3 = jest.fn();
    bus.onMessage(onMessage3);

    globalMockWs.simulateServerConnected();
    globalMockWs.simulateIncomingMessage('incoming message');

    expect(onMessage1).toHaveBeenCalledWith(expect.objectContaining({data: 'incoming message'}));
    expect(onMessage2).toHaveBeenCalledWith(expect.objectContaining({data: 'incoming message'}));
    expect(onMessage3).toHaveBeenCalledWith(expect.objectContaining({data: 'incoming message'}));

    disposable2.dispose();

    globalMockWs.simulateIncomingMessage('another after dispose');
    expect(onMessage2).not.toHaveBeenCalledWith(
      expect.objectContaining({data: 'another after dispose'}),
    );
    // the other handlers still active
    expect(onMessage1).toHaveBeenCalledWith(
      expect.objectContaining({data: 'another after dispose'}),
    );
    expect(onMessage3).toHaveBeenCalledWith(
      expect.objectContaining({data: 'another after dispose'}),
    );
  });

  it('can send messages as soon as connection is created', () => {
    const bus = createMessageBus();
    bus.onChangeStatus(newStatus => {
      if (newStatus.type === 'open') {
        bus.postMessage('message once connected');
      }
    });
    globalMockWs.simulateServerConnected();

    expect(globalMockWs.sentMessages).toEqual(['message once connected']);
  });

  it('includes token from initialState', () => {
    createMessageBus();
    expect(globalMockWs.url).toEqual(`ws://${DEFAULT_HOST}/ws?token=1234`);
  });

  describe('reconnect timing', () => {
    beforeEach(() => {
      jest.useFakeTimers();
    });
    afterEach(() => {
      jest.useRealTimers();
    });

    it('reconnects after a delay', () => {
      createMessageBus();

      const initialWs = globalMockWs;

      globalMockWs.simulateServerConnected();
      globalMockWs.simulateServerDisconnected();
      expect(initialWs).toBe(globalMockWs);
      jest.runAllTimers();
      // we have a new WebSocket instance which will re-try to connect
      expect(initialWs).not.toBe(globalMockWs);
    });

    it("doesn't reconnect after disposing", () => {
      const bus = createMessageBus();

      const previousWs = globalMockWs;
      globalMockWs.simulateServerConnected();
      bus.dispose();
      globalMockWs.simulateServerDisconnected();
      expect(previousWs).toBe(globalMockWs);
      jest.runAllTimers();
      expect(previousWs).toBe(globalMockWs); // we haven't made a new WebSocket, because we didn't try to reconnect
    });

    it('reconnects with exponential backoff', () => {
      createMessageBus();

      const initialWs = globalMockWs;

      globalMockWs.simulateServerConnected();
      globalMockWs.simulateServerDisconnected();
      expect(initialWs).toBe(globalMockWs);
      jest.advanceTimersByTime(LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS + 10);
      expect(initialWs).not.toBe(globalMockWs);

      const nextWs = globalMockWs;

      // we failed to connect again
      globalMockWs.simulateServerDisconnected();
      // with exponential backoff, we should need to wait another tick
      jest.advanceTimersByTime(LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS + 10);
      expect(nextWs).toBe(globalMockWs);
      // but after another round, we're past the doubled time
      jest.advanceTimersByTime(LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS + 10);
      expect(nextWs).not.toBe(globalMockWs);
    });

    it('resets exponential backoff after a successful connection', () => {
      createMessageBus();

      globalMockWs.simulateServerConnected();

      // simulate 2 disconnects, which doubles backoff time
      globalMockWs.simulateServerDisconnected();
      jest.advanceTimersByTime(LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS + 10);
      globalMockWs.simulateServerDisconnected();
      jest.advanceTimersByTime(2 * LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS + 10);

      // now reconnect should reset backoff time
      globalMockWs.simulateServerConnected();

      const initialWs = globalMockWs;
      globalMockWs.simulateServerDisconnected();
      expect(initialWs).toBe(globalMockWs);
      // advancing by initial reconnect time creates a new ws
      jest.advanceTimersByTime(LocalWebSocketEventBus.DEFAULT_RECONNECT_CHECK_TIME_MS + 10);
      expect(initialWs).not.toBe(globalMockWs);
    });

    it('caps out exponential backoff at maximum', () => {
      createMessageBus();

      globalMockWs.simulateServerConnected();

      // simulate a bunch of unsucessful reconnects over time
      for (let i = 0; i < 100; i++) {
        globalMockWs.simulateServerDisconnected();
        jest.advanceTimersByTime(LocalWebSocketEventBus.MAX_RECONNECT_CHECK_TIME_MS);
      }

      // now backoff time should have stopped doubling

      const initialWs = globalMockWs;
      globalMockWs.simulateServerDisconnected();
      expect(initialWs).toBe(globalMockWs);
      // advancing by anything less than cap of reconnect time doesn't reconnect yet
      jest.advanceTimersByTime(LocalWebSocketEventBus.MAX_RECONNECT_CHECK_TIME_MS - 10);
      expect(initialWs).toBe(globalMockWs);

      // but just a little further pushes us over the edge and we reconnect
      jest.advanceTimersByTime(20);
      expect(initialWs).not.toBe(globalMockWs);
    });
  });
});
