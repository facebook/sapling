/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {BroadcastMessage} from './broadcast';
import type {
  DiffAndTokenizeParams,
  DiffAndTokenizeResponse,
  LineRangeParams,
  LineRangeResponse,
  LineToPositionParams,
  Message,
  Response,
  Result,
} from './diffServiceWorker';
import type {LineToPosition} from './lineToPosition';
import type {SupportedPrimerColorMode} from './themeState';

import {
  AVAILABILITY_METHOD,
  createDiffServiceBroadcastChannel,
  createWorkerName,
  parseWorkerIndex,
} from './broadcast';
import {atom, selector, selectorFamily} from 'recoil';
import {unwrap} from 'shared/utils';

export const lineToPosition = selectorFamily<LineToPosition, LineToPositionParams>({
  key: 'lineToPosition',
  get:
    (params: LineToPositionParams) =>
    ({get}) => {
      const worker = get(diffServiceClient);
      const message: Message = {
        id: worker.nextID(),
        method: 'lineToPosition',
        params,
      };
      return worker.sendMessage(message) as Promise<LineToPosition>;
    },
});

/**
 * Request the colorMap to use with the specified SupportedPrimerColorMode for
 * use with `updateTextMateGrammarCSS()`.
 */
export const colorMap = selectorFamily<string[], SupportedPrimerColorMode>({
  key: 'colorMap',
  get:
    (colorMode: SupportedPrimerColorMode) =>
    ({get}) => {
      const worker = get(diffServiceClient);
      const message: Message = {
        id: worker.nextID(),
        method: 'colorMap',
        params: {colorMode},
      };
      return worker.sendMessage(message) as Promise<string[]>;
    },
});

export const diffAndTokenize = selectorFamily<DiffAndTokenizeResponse, DiffAndTokenizeParams>({
  key: 'diffAndTokenize',
  get:
    (params: DiffAndTokenizeParams) =>
    ({get}) => {
      const worker = get(diffServiceClient);
      const message: Message = {
        id: worker.nextID(),
        method: 'diffAndTokenize',
        params,
      };
      return worker.sendMessage(message) as Promise<DiffAndTokenizeResponse>;
    },
});

export const lineRange = selectorFamily<string[], LineRangeParams>({
  key: 'lineRange',
  get:
    (params: LineRangeParams) =>
    ({get}) => {
      const worker = get(diffServiceClient);
      const message: Message = {
        id: worker.nextID(),
        method: 'lineRange',
        params,
      };
      const promise = worker.sendMessage(message) as Promise<LineRangeResponse>;
      return promise.then(({unsplitLines, notFound, isBinary}) => {
        if (unsplitLines != null) {
          return unsplitLines.split('\n');
        } else if (notFound) {
          // eslint-disable-next-line no-console
          console.error(`blob ${params.oid} not found for lineRange`);
        } else if (isBinary) {
          // eslint-disable-next-line no-console
          console.error(`blob ${params.oid} is binary, no lineRange`);
        }
        return [];
      });
    },
});

/**
 * Client that is paired with an instance of `diffServiceWorker`. Takes
 * responsibility for pairing requests and responses to the Web Worker, making
 * the result available as a Promise to the caller.
 */
class DiffServiceClient {
  private worker: SharedWorker;
  private pendingRequests: Map<number, (result: Result) => void> = new Map();

  constructor(workerName: string) {
    this.worker = new SharedWorker(new URL('./diffServiceWorker.ts', import.meta.url), {
      name: workerName,
    });
    this.worker.port.onmessage = event => this.onmessage(event);
    // eslint-disable-next-line no-console
    this.worker.port.onmessageerror = event => console.error(event);
  }

  private onmessage({data}: {data: Response}) {
    const {id, ok, err} = data;
    const handler = this.pendingRequests.get(id);
    if (handler == null) {
      // eslint-disable-next-line no-console
      console.error(`no handler found for ${id}: multiple responses sent?`);
      return;
    }

    this.pendingRequests.delete(id);
    handler({ok, err});
  }

  private once(id: number, onresponse: (response: Result) => void) {
    this.pendingRequests.set(id, onresponse);
  }

  sendMessage(message: Message): Promise<unknown> {
    const {id} = message;
    const promise = new Promise((resolve, reject) => {
      this.once(id, (result: Result) => {
        const {ok, err} = result;
        if (err != null) {
          reject(err);
        } else {
          resolve(ok);
        }
      });
    });
    this.worker.port.postMessage(message);
    return promise;
  }
}

/** This might be too many, but we'll try it out... */
const MAX_SERVICE_WORKERS = 6;

class WorkerPool {
  /** Used to get updates about the availability of SharedWorkers. */
  private broadcast: BroadcastChannel;

  /**
   * Due to the nature of how notifications from the BroadastChannel work, it
   * is possible for the workers array to contain "holes" if, for example, the
   * first availability notification is for worker #2 and then this.workers[2]
   * will be set, but [0] and [1] will be undefined.
   */
  private workers: Array<undefined | {client: DiffServiceClient; available: boolean}> = [];

  /**
   * Messages that are waiting for a SharedWorker to become available in order
   * to be sent. The response from sendMessage() should be passed to resolve or
   * reject, as appropriate.
   */
  private pendingMessages: {
    message: Message;
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
  }[] = [];

  /**
   * Used by this.nextID() to ensure each message sent from the pool gets a
   * unique ID so it can be paired with the response.
   */
  private requestID = 0;

  constructor() {
    this.broadcast = createDiffServiceBroadcastChannel();
    this.broadcast.onmessage = (event: MessageEvent) => this.onBroadcastMessageReceived(event);
  }

  private onBroadcastMessageReceived({data}: MessageEvent) {
    const message = data as BroadcastMessage;
    if (message.method !== AVAILABILITY_METHOD) {
      return;
    }

    const {workerName, available} = message;
    const index = parseWorkerIndex(workerName);
    if (index == null) {
      // eslint-disable-next-line no-console
      console.error(`could not parse worker index: ${workerName}`);
      return;
    }

    const worker = this.workers[index];
    if (worker !== undefined) {
      worker.available = available;
    } else {
      this.workers[index] = {client: new DiffServiceClient(workerName), available};
    }

    if (available && this.pendingMessages.length > 0) {
      this.trySendingPendingMessage();
    }
  }

  sendMessage(message: Message): Promise<unknown> {
    // For now, we use a simple round-robin scheduler.
    // We could certainly do much better here:
    // - keeping track of idle workers when deciding who to assign to
    // - resizing the pool based on demand
    // - worker affinity based on scopeName
    const client = this.findAvailableClient();
    if (client != null) {
      return client.sendMessage(message);
    }

    // No available workers! Add the message to the `pendingMessages` list and
    // request a new one if we haven't hit MAX_SERVICE_WORKERS.
    let resolve: ((value: unknown) => void) | null = null;
    let reject: ((error: Error) => void) | null = null;
    const promise = new Promise((_resolve, _reject) => {
      resolve = _resolve;
      reject = _reject;
    });

    this.pendingMessages.push({
      message,
      resolve: unwrap<(value: unknown) => void>(resolve),
      reject: unwrap<(error: Error) => void>(reject),
    });

    // Note it is possible that there are "holes" in this.workers, e.g.,
    // this.workers[0] is undefined while this.workers[1] is set, but
    // unavailable. In all likelihood, this.workers[0] *exists*, but we have
    // not received an initial availability update yet. If this is the case, we
    // try to fill the hole before extending this.workers.
    let workerIndex = this.workers.findIndex(val => val === undefined);
    if (workerIndex === -1) {
      const numWorkers = this.workers.length;
      if (numWorkers < MAX_SERVICE_WORKERS) {
        workerIndex = numWorkers;
      }
    }

    if (workerIndex !== -1) {
      const workerName = createWorkerName(workerIndex);
      // While it is possible that the ServiceWorker was already created by
      // another browser tab and is available, we initially assume it is
      // unavailable, but we request it to publish its availability.
      const client = new DiffServiceClient(workerName);
      this.workers[workerIndex] = {client, available: false};
      client.sendMessage({method: 'publishAvailabilty', id: -1, params: null});
    }

    return promise;
  }

  private trySendingPendingMessage(): void {
    const client = this.findAvailableClient();
    if (client == null) {
      return;
    }

    const pendingMessage = this.pendingMessages.shift();
    if (pendingMessage === undefined) {
      return;
    }

    const {message, resolve, reject} = pendingMessage;
    client.sendMessage(message).then(resolve, reject);
  }

  private findAvailableClient(): DiffServiceClient | null {
    for (const worker of this.workers) {
      if (worker !== undefined && worker.available) {
        return worker.client;
      }
    }
    return null;
  }

  nextID(): number {
    return ++this.requestID;
  }
}

const diffServiceClient = atom<WorkerPool>({
  key: 'diffServiceClient',
  default: selector({
    key: 'diffServiceClient/default',
    get: () => new WorkerPool(),
    dangerouslyAllowMutability: true,
  }),
  // WorkerPool contains mutable collections.
  dangerouslyAllowMutability: true,
});
