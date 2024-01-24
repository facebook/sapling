/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Snapshot} from 'recoil';
import type {Json} from 'shared/typeUtils';

export type UIStateSnapshot = {[key: string]: Json};

/**
 * Dump all recoil atoms into JSON format for manual inspection.
 * Note: not all values we store will be serializable, but those are just replaced with an error string.
 */
export function getAllRecoilStateJson(snapshot: Snapshot): UIStateSnapshot {
  function trySerialize(s: () => Json): Json {
    try {
      return s();
    } catch {
      return 'Error parsing recoil state';
    }
  }
  const resolvedNodes = [...snapshot.getNodes_UNSTABLE()].map((node): [string, Json] => {
    const loadable = snapshot.getLoadable(node);
    const value =
      loadable.state === 'hasValue'
        ? (loadable.valueMaybe() as Json)
        : loadable.state === 'loading'
        ? '(pending promise)'
        : loadable.errorMaybe();
    return [
      node.key,
      shouldSkipField(node.key) ? '(skipped)' : trySerialize(() => serialize(value)),
    ];
  });
  return Object.fromEntries(resolvedNodes);
}

/** If we included the entire UI state, it would be several MB large and hard to read.
 * Let's trim down some unnecessary fields, such as large selectors that derive from other state. */
function shouldSkipField(key: string): boolean {
  return (
    // all commits already listed in latestCommitsData, these selectors are convenient in code but provide no additional info
    key === 'latestCommitTreeMap' ||
    key === 'latestCommits' ||
    key === 'linearizedCommitHistory' ||
    key.startsWith('commitByHash') ||
    // available in allDiffSummaries
    key.startsWith('diffSummary')
  );
}

function serialize(arg: Json): Json {
  if (arg === undefined) {
    return null;
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
    return Array.from(arg.entries()).map(([key, val]) => [serialize(key), serialize(val)]);
  } else if (arg instanceof Set) {
    return Array.from(arg.values()).map(serialize);
  } else if (arg instanceof Error) {
    return {message: arg.message ?? null, stack: arg.stack ?? null};
  } else if (arg instanceof Date) {
    return `Date: ${arg.valueOf()}`;
  } else if (Array.isArray(arg)) {
    return arg.map(a => serialize(a));
  } else if (typeof arg === 'object') {
    const newObj: Json = {};
    for (const [propertyName, propertyValue] of Object.entries(arg)) {
      newObj[propertyName] = serialize(propertyValue);
    }

    return newObj;
  }

  throw new Error(`cannot serialize argument ${arg}`);
}
