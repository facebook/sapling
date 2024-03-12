/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {Atom} from 'jotai';
import type {Json} from 'shared/typeUtils';

import {latestSuccessorsMapAtom} from '../SuccessionTracker';
import {allDiffSummaries, codeReviewProvider} from '../codeReview/CodeReviewInfo';
import {readAtom} from '../jotaiUtils';
import {uncommittedSelection} from '../partialSelection';
import {selectedCommits} from '../selection';
import {
  operationBeingPreviewed,
  repositoryData,
  latestCommitsData,
  latestUncommittedChangesData,
  mergeConflicts,
  operationList,
  queuedOperations,
} from '../serverAPIState';
import {SelfUpdate} from 'shared/immutableExt';

export type UIStateSnapshot = {[key: string]: Json};
export type AtomsState = {[key: string]: unknown};

function listInterestingAtoms(): Array<Atom<unknown>> {
  return [
    allDiffSummaries,
    codeReviewProvider,
    repositoryData,
    latestCommitsData,
    latestSuccessorsMapAtom,
    latestUncommittedChangesData,
    mergeConflicts,
    operationBeingPreviewed,
    operationList,
    queuedOperations,
    selectedCommits,
    uncommittedSelection,
    // This is an atomFamily. Need extra work to read it.
    // unsavedFieldsBeingEdited,
  ];
}

/** Read all "interesting" atoms and returns a single object that contains them all. */
export function readInterestingAtoms(): AtomsState {
  return Object.fromEntries(
    listInterestingAtoms().map(a => [a.debugLabel ?? a.toString(), readAtom(a)]),
  );
}

/** Try to serialize the `state` so they can be represented in plain JSON. */
export function serializeAtomsState(state: AtomsState): UIStateSnapshot {
  const newEntries = Object.entries(state).map(([key, value]) => {
    return [key, serialize(value as Serializable)];
  });
  return Object.fromEntries(newEntries);
}

type Serializable = Json | {toJSON: () => Serializable};

function serialize(initialArg: Serializable): Json {
  let arg = initialArg;

  const isObject = arg != null && typeof arg === 'object';

  // Extract debug state provided by the object. This applies to both immutable and regular objects.
  // This needs to happen before unwrapping SelfUpdate.
  let debugState = null;
  if (isObject) {
    // If the object defines `getDebugState`. Call it to get more (easier to visualize) states.
    const maybeGetDebugState = (arg as {getDebugState?: () => {[key: string]: Json}}).getDebugState;
    if (maybeGetDebugState != null) {
      debugState = maybeGetDebugState.call(arg);
    }
  }

  // Unwrap SelfUpdate types.
  if (arg instanceof SelfUpdate) {
    arg = arg.inner;
  }

  // Convert known immutable types.
  if (arg != null && typeof arg === 'object') {
    const maybeToJSON = (arg as {toJSON?: () => Json}).toJSON;
    if (maybeToJSON !== undefined) {
      arg = maybeToJSON.call(arg);
      if (typeof arg === 'object' && debugState != null) {
        arg = {...debugState, ...arg};
      }
    }
  }

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
    const newObj: Json = debugState ?? {};
    for (const [propertyName, propertyValue] of Object.entries(arg)) {
      // Skip functions.
      if (typeof propertyValue === 'function') {
        continue;
      }
      newObj[propertyName] = serialize(propertyValue);
    }

    return newObj;
  }

  // Return a dummy value instead of throw so if an item in a container is "bad",
  // it does not turn the whole container into an error.
  return `<unserializable: ${arg}>`;
}
