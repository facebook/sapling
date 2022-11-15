/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**
 * Types that are compatible with JSON.stringify and can be sent over the transport,
 * Plus Map, Set, Date, Error are supported (they are converted to objects before serializing)
 */
export type Serializable =
  | string
  | number
  | boolean
  | null
  | undefined
  | Map<Serializable, Serializable>
  | Set<Serializable>
  | Error
  | Date
  | {[key: string]: Serializable}
  | Array<Serializable>;

export type Serialized =
  | string
  | number
  | boolean
  | null
  | undefined
  | Array<Serialized>
  | CustomSerialized;

export type CustomSerialized =
  | {__rpcType: 'undefined'}
  | {__rpcType: 'object'; [key: string]: Serialized}
  | {__rpcType: 'Error'; data: {message: string; stack?: string}}
  | {__rpcType: 'Map'; data: Array<[Serialized, Serialized]>}
  | {__rpcType: 'Set'; data: Array<Serialized>}
  | {__rpcType: 'Date'; data: number};

const UNDEFINED_SERIALIZED = {__rpcType: 'undefined' as const};

/**
 * Prepare function arguments/return value to be serialized. This lets you pass Map/Set/RegExp to rpc functions.
 * Note that we need to do this recursively for arguments to Map/Set, since you can have complex nesting like Map<Set<>, Map<>>
 */
export function serialize(arg: Serializable): Serialized {
  // 'undefined' is not valid JSON, so it will be converted to 'null' when serialized
  // Therefore, we must serialize it ourselves
  if (arg === undefined) {
    return UNDEFINED_SERIALIZED;
  }

  if (
    typeof arg === 'number' ||
    typeof arg === 'boolean' ||
    typeof arg === 'string' ||
    arg === null
  ) {
    return arg;
  }

  if (arg instanceof Map) {
    return {
      __rpcType: 'Map',
      data: Array.from(arg.entries()).map(([key, val]) => [serialize(key), serialize(val)]),
    } as CustomSerialized;
  } else if (arg instanceof Set) {
    return {__rpcType: 'Set', data: Array.from(arg.values()).map(serialize)} as CustomSerialized;
  } else if (arg instanceof Error) {
    return {__rpcType: 'Error', data: {message: arg.message, stack: arg.stack}} as CustomSerialized;
  } else if (arg instanceof Date) {
    return {__rpcType: 'Date', data: arg.valueOf()} as CustomSerialized;
  } else if (Array.isArray(arg)) {
    return arg.map(a => serialize(a));
  } else if (typeof arg === 'object') {
    const newObj: CustomSerialized & {__rpcType: 'object'} = {__rpcType: 'object'};
    for (const [propertyName, propertyValue] of Object.entries(arg)) {
      newObj[propertyName] = serialize(propertyValue);
    }

    return newObj;
  }

  throw new Error(`cannot serialize argument ${arg}`);
}

export function serializeToString(data: Serializable): string {
  return JSON.stringify(serialize(data));
}

/**
 * Restore function arguments/return value after deserializing. This lets you recover passed Map/Set/Date/Error during remote transport.
 */
export function deserialize(arg: Serialized): Serializable {
  if (typeof arg !== 'object' || arg == null) {
    return arg;
  }

  if (Array.isArray(arg)) {
    return arg.map(a => deserialize(a));
  }

  const specific = arg as CustomSerialized;
  switch (specific.__rpcType) {
    case 'undefined':
      return undefined;
    case 'Map':
      return new Map(specific.data.map(([key, value]) => [deserialize(key), deserialize(value)]));
    case 'Set':
      return new Set(specific.data.map(deserialize));
    case 'Error': {
      const e = new Error();
      e.stack = specific.data.stack;
      e.message = specific.data.message;
      return e;
    }
    case 'Date':
      return new Date(specific.data);
    case 'object': {
      const standardObject = arg as {[key: string]: Serialized};
      const newObj: {[key: string]: Serializable} = {};
      for (const [propertyName, propertyValue] of Object.entries(standardObject)) {
        if (propertyName !== '__rpcType') {
          newObj[propertyName] = deserialize(propertyValue);
        }
      }
      return newObj;
    }
    default: {
      throw new Error(`cannot deserialize unknown type ${specific}`);
    }
  }
}

export function deserializeFromString(data: string): Serializable {
  return deserialize(JSON.parse(data));
}
