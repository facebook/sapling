/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import lazyInit from '../lazyInit';

describe('lazyInit', () => {
  test('async operation never called', () => {
    let numCalls = 0;
    let theObject;
    function expensiveObjCreation() {
      ++numCalls;
      theObject = {};
      return Promise.resolve(theObject);
    }

    const getObj = lazyInit(expensiveObjCreation);
    expect(typeof getObj).toBe('function');
    expect(theObject).toBe(undefined);
    expect(numCalls).toBe(0);
  });

  test('async operation called when value requested', async () => {
    let numCalls = 0;
    let theObject;
    function expensiveObjCreation() {
      ++numCalls;
      theObject = {};
      return Promise.resolve(theObject);
    }

    const getObj = lazyInit(expensiveObjCreation);
    expect(numCalls).toBe(0);
    const obj = await getObj();
    expect(numCalls).toBe(1);
    expect(obj).toBe(theObject);
  });

  test('async operation called only once when value requested many times', async () => {
    let numCalls = 0;
    let theObject;
    function expensiveObjCreation() {
      ++numCalls;
      theObject = {};
      return Promise.resolve(theObject);
    }

    const getObj = lazyInit(expensiveObjCreation);
    expect(numCalls).toBe(0);

    const obj1 = await getObj();
    const obj2 = await getObj();
    const obj3 = await getObj();
    expect(numCalls).toBe(1);
    expect(obj1).toBe(theObject);
    expect(obj2).toBe(theObject);
    expect(obj3).toBe(theObject);
  });
});
