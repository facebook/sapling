/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

class EventWithPayload<T> extends Event {
  constructor(
    type: string,
    public data: T | Error,
  ) {
    super(type);
  }
}

type TypedListenerParam<T> = ((data: T) => void) | ((err: Error) => void);

/**
 * Like {@link EventEmitter} / {@link EventTarget}, but with type checking for one particular subscription type,
 * plus errors. uses EventTarget so it works in browser and node.
 * ```
 * const myEmitter = new TypedEventEmitter<'data', number>();
 * myEmitter.on('data', (data: number) => ...); // typechecks 'data' param and callback
 * myEmitter.on('error', (error: Error) => ...); // errors are always allowed too
 * // Fields other than 'data' and 'error' are type errors.
 * ```
 */
export class TypedEventEmitter<EventName extends string, EventType> {
  private listeners: {[key: string]: Map<TypedListenerParam<EventType>, EventListener>} = {};

  private target = new EventTarget();

  on(event: EventName, listener: (data: EventType) => void): this;
  on(event: 'error', listener: (err: Error) => void): this;
  on(event: EventName | 'error', listener: TypedListenerParam<EventType>): this {
    const map = this.getOrCreateMap(event);
    let found = map.get(listener);
    if (found == null) {
      found = (event: Event | EventWithPayload<EventType>) => {
        if ('data' in event) {
          (listener as (data: EventType | Error) => void)(event.data);
        }
      };
      map.set(listener, found);
    }
    this.target.addEventListener(event as EventName | 'error', found);
    return this;
  }

  off(event: EventName, listener: (data: EventType) => void): this;
  off(event: 'error', listener: (err: Error) => void): this;
  off(event: EventName | 'error', listener: TypedListenerParam<EventType>): this {
    const map = this.getOrCreateMap(event);
    const found = map.get(listener);
    if (found == null) {
      return this;
    }
    map.delete(listener);
    this.target.removeEventListener(event as EventName | 'error', found);
    return this;
  }

  emit(event: EventName): EventType extends undefined ? boolean : never;
  emit(event: EventName, data: EventType): boolean;
  emit(event: 'error', data: Error): boolean;

  emit(name: EventName | 'error', data?: EventType | Error): boolean {
    const event = new EventWithPayload(name, data);
    if (!this.target.dispatchEvent(event)) {
      return false;
    }
    return true;
  }

  private getOrCreateMap(event: EventName | 'error') {
    return (this.listeners[event] ??= new Map());
  }

  removeAllListeners() {
    for (const [key, map] of Object.entries(this.listeners)) {
      for (const listener of map.values()) {
        this.target.removeEventListener(key, listener);
      }
      map.clear();
    }
  }

  /** Get an EventTarget-compatible object while still being able to use the typed APIs */
  asEventTarget(): EventTarget {
    const listeners = new Map<(event: Event) => void, (data: EventType) => void>();
    return {
      addEventListener: (type: EventName, handler: (event: Event) => void) => {
        const wrapped = (data: EventType) => {
          handler(new EventWithPayload<EventType>(type, data));
        };
        listeners.set(handler, wrapped);
        this.on(type as EventName, wrapped);
      },
      removeEventListener: (type: EventName, handler: (event: Event) => void) => {
        const existing = listeners.get(handler);
        if (existing) {
          this.off(type as EventName, existing);
          listeners.delete(handler);
        }
      },
      dispatchEvent: (event: EventWithPayload<EventType>) =>
        this.emit(event.type as EventName, event.data as EventType),
    } as EventTarget;
  }
}
