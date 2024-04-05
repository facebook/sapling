/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {CancellationToken} from 'shared/CancellationToken';

type WorkerModuleType<Request, Response> = {
  handleMessage: (callback: (msg: Response) => void, msg: MessageEvent<Request>) => void;
};
/**
 * Some environments, like jest tests, don't support WebWorkers.
 * Let's just import the equivalent file dynamically and call the functions
 * instead of doing message passing, so we can validate syntax highlighting in tests.
 */
export class SynchronousWorker<Request, Response> {
  private importedModulePromise: Promise<WorkerModuleType<Request, Response>> | undefined;
  constructor(
    private getImportedModulePromise: () => Promise<WorkerModuleType<Request, Response>>,
  ) {}

  public getImportedModule(): Promise<WorkerModuleType<Request, Response>> {
    if (this.importedModulePromise) {
      return this.importedModulePromise;
    }
    this.importedModulePromise = this.getImportedModulePromise();
    return this.importedModulePromise;
  }

  public onmessage = (_e: MessageEvent) => null;
  public postMessage(msg: Request) {
    this.getImportedModule().then(module => {
      module.handleMessage(
        (msg: Response) => {
          this.onmessage({data: msg} as MessageEvent<Response>);
        },
        {data: msg} as MessageEvent<Request>,
      );
    });
  }

  public dispose(): void {
    return undefined;
  }
}

export class WorkerApi<Request extends {type: string}, Response extends {type: string}> {
  private id = 0;
  private requests = new Map<number, (response: Response) => void>();
  private listeners = new Map<Response['type'], (msg: Response) => void>();

  constructor(public worker: Worker) {
    type ResponseWithId = Response & {id: number};
    this.worker.onmessage = e => {
      const msg = e.data as Response;
      const id = (msg as ResponseWithId).id;
      const callback = this.requests.get(id);
      if (callback) {
        callback(msg);
        this.requests.delete(id);
      }

      const listener = this.listeners.get(msg.type);
      if (listener) {
        listener(msg);
      }
    };
  }

  /** Send a message, then wait for a reply */
  request<T extends Request['type']>(
    msg: Request & {type: T},
    cancellationToken?: CancellationToken,
  ): Promise<Response & {type: T}> {
    return new Promise<Response & {type: T}>(resolve => {
      const id = this.id++;
      this.worker.postMessage({...msg, id});

      const disposeOnCancel = cancellationToken?.onCancel(() => {
        this.worker.postMessage({type: 'cancel', idToCancel: id});
      });
      this.requests.set(id, result => {
        (resolve as (response: Response) => void)(result);
        disposeOnCancel?.();
      });
    });
  }

  /** listen for messages from the server of a given type */
  listen<T extends Response['type']>(
    type: T,
    listener: (msg: Response & {type: T}) => void,
  ): () => void {
    this.listeners.set(type, listener as (msg: Response) => void);
    return () => this.listeners.delete(type);
  }

  dispose() {
    this.worker.terminate();
  }
}
