/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {ValueObject} from 'immutable';

const IS_RECORD_SYMBOL = '@@__IMMUTABLE_RECORD__@@';

/** Wraps a ValueObject so it self-updates on equals. */
export class SelfUpdate<T extends ValueObject> implements ValueObject {
  inner: T;

  /**
   * Tell Recoil to not deepFreeze (Object.seal) this object. This is needed
   * since we might update the `inner` field. We didn't break Recoil
   * assumptions since we maintain the same "value" of the object.
   *
   * See https://github.com/facebookexperimental/Recoil/blob/0.7.7/packages/shared/util/Recoil_deepFreezeValue.js#L42
   * Recoil tests `value[IS_RECORD_SYMBOL] != null`.
   *
   * For immutable.js, it actually checks the boolean value.
   * See https://github.com/immutable-js/immutable-js/blob/v4.3.4/src/predicates/isRecord.js
   * Immutable.js uses `Boolean(maybeRecord && maybeRecord[IS_RECORD_SYMBOL])`.
   *
   * By using `false`, this tricks Recoil to treat us as an immutable value,
   * while does not break Immutable.js' type checking.
   */
  [IS_RECORD_SYMBOL] = false;

  constructor(inner: T) {
    this.inner = inner;
  }

  hashCode(): number {
    return this.inner.hashCode() + 1;
  }

  equals(other: unknown): boolean {
    if (!(other instanceof SelfUpdate)) {
      return false;
    }
    if (this === other) {
      return true;
    }
    const otherInner = other.inner;
    const result = this.inner.equals(otherInner);
    if (result && this.inner !== otherInner) {
      this.inner = otherInner;
    }
    return result;
  }
}
