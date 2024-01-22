/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {LRUWithStats} from '../LRU';

import {cached, LRU, clearTrackedCache} from '../LRU';
import {SelfUpdate} from '../immutableExt';
import {List, Record} from 'immutable';

describe('LRU', () => {
  it('evicts oldest items after reaching the max', () => {
    const lru = new LRU(3);
    lru.set(1, 1);
    lru.set(2, 2);
    lru.set(3, 3);
    lru.set(4, 4);
    expect(lru.get(4)).toEqual(4);
    expect(lru.get(3)).toEqual(3);
    expect(lru.get(2)).toEqual(2);
    expect(lru.get(1)).toEqual(undefined);
  });

  it('refreshes items on get', () => {
    const lru = new LRU(3);
    lru.set(1, 1);
    lru.set(2, 2);
    lru.set(3, 3);
    expect(lru.get(1)).toEqual(1);
    lru.set(4, 4);
    expect(lru.get(2)).toEqual(undefined);
    expect(lru.get(3)).toEqual(3);
    expect(lru.get(4)).toEqual(4);
  });

  it('refreshes items on set', () => {
    const lru = new LRU(3);
    lru.set(1, 1);
    lru.set(2, 2);
    lru.set(3, 3);
    lru.set(4, 4);
    lru.set(1, 1.1);
    expect(lru.get(4)).toEqual(4);
    expect(lru.get(3)).toEqual(3);
    expect(lru.get(2)).toEqual(undefined);
    expect(lru.get(1)).toEqual(1.1);
  });

  it('can delete items', () => {
    const lru = new LRU(3);
    lru.set(1, 1);
    lru.set(2, 2);
    lru.set(3, 3);
    lru.delete(3);
    lru.set(4, 4);
    expect(lru.get(4)).toEqual(4);
    expect(lru.get(3)).toEqual(undefined);
    expect(lru.get(2)).toEqual(2);
    expect(lru.get(1)).toEqual(1);
  });

  it('allows storing falsey values', () => {
    const lru = new LRU(8);
    lru.set(1, null);
    lru.set(2, undefined);
    lru.set(3, '');
    lru.set(4, false);
    const emptyArray: Array<number> = [];
    lru.set(5, emptyArray);
    const emptyObject = {};
    lru.set(6, emptyObject);

    expect(lru.get(1)).toBe(null);
    expect(lru.get(2)).toBe(undefined);
    expect(lru.get(3)).toBe('');
    expect(lru.get(4)).toBe(false);
    expect(lru.get(5)).toBe(emptyArray);
    expect(lru.get(6)).toBe(emptyObject);
  });

  it('allows falsey keys', () => {
    const lru = new LRU(8);
    lru.set(null, 1);
    lru.set(undefined, 2);
    lru.set('', 3);
    lru.set(false, 4);
    const emptyArray: Array<number> = [];
    lru.set(emptyArray, 5);
    const emptyObject = {};
    lru.set(emptyObject, 6);

    expect(lru.get(null)).toBe(1);
    expect(lru.get(undefined)).toBe(2);
    expect(lru.get('')).toBe(3);
    expect(lru.get(false)).toBe(4);
    expect(lru.get(emptyArray)).toBe(5);
    expect(lru.get(emptyObject)).toBe(6);
  });

  it('undefined keys are evictable', () => {
    const lru = new LRU(2);
    lru.set(undefined, 1);
    lru.set(2, 2);
    lru.set(3, 3);

    expect(lru.get(undefined)).toBe(undefined);
  });

  it('setting an undefined value does not take space in the cache', () => {
    const lru = new LRU(2);
    lru.set(1, undefined);
    lru.set(2, null);
    lru.set(3, undefined);
    lru.set(4, null);
    lru.set(5, undefined);
    lru.set(6, undefined);

    expect(lru.get(1)).toBe(undefined);
    expect(lru.get(2)).toBe(null);
    expect(lru.get(3)).toBe(undefined);
    expect(lru.get(4)).toBe(null);
    expect(lru.get(5)).toBe(undefined);
    expect(lru.get(6)).toBe(undefined);
  });

  it('works with SelfUpdate keys to avoid repeatitive deepEquals', () => {
    const ListEqualsMock = jest.spyOn(List.prototype, 'equals');

    type NestedList = List<number | NestedList>;
    const nestedList = (level: number): NestedList =>
      level <= 0 ? List([10]) : List([nestedList(level - 1)]);

    const list1 = new SelfUpdate(nestedList(10));
    const list2 = new SelfUpdate(nestedList(10));

    const lru = new LRU(1);
    lru.set(list1, 'x');

    expect(lru.get(list2)).toBe('x');
    expect(ListEqualsMock).toHaveBeenCalledTimes(11);
    ListEqualsMock.mockClear();

    // SelfUpdate avoids deepEquals after the first lru.get().
    expect(lru.get(list2)).toBe('x');
    expect(ListEqualsMock).toHaveBeenCalledTimes(1);
    ListEqualsMock.mockClear();

    // SelfUpdate can be used in a nested structure.
    const list3 = List([List([new SelfUpdate(nestedList(8))])]);
    const list4 = List([List([new SelfUpdate(nestedList(8))])]);

    lru.set(list3, 'y');
    expect(lru.get(list4)).toBe('y');
    expect(ListEqualsMock).toHaveBeenCalledTimes(11);
    ListEqualsMock.mockClear();

    expect(lru.get(list4)).toBe('y');
    expect(ListEqualsMock).toHaveBeenCalledTimes(3);
    ListEqualsMock.mockClear();

    ListEqualsMock.mockRestore();
  });
});

describe('LRU benchmark', () => {
  const getLru = new LRU(1000);
  const n = 1000;
  for (let i = 0; i < n; i++) {
    getLru.set(List([i]), i);
  }

  it('get() with a large cache size', () => {
    for (let j = 0; j < 100; j++) {
      let equalCount = 0;
      for (let i = 0; i < n; i++) {
        const item = getLru.get(List([i]));
        equalCount += item === i ? 1 : 0;
      }
      expect(equalCount).toBe(n);
    }
  });

  it('set() with a large cache size', () => {
    const setLru = new LRU(1000);
    for (let j = 0; j < 100; j++) {
      for (let i = 0; i < n; i++) {
        setLru.set(List([i]), i);
      }
    }
  });
});

describe('cached()', () => {
  describe('for pure functions', () => {
    it('works for pure function', () => {
      let calledTimes = 0;
      const fib = cached((n: number): number => {
        calledTimes += 1;
        return n < 2 ? n : fib(n - 1) + fib(n - 2);
      });
      expect(fib(20)).toBe(6765);
      expect(calledTimes).toBe(21);
    });

    it('takes user-provided cache', () => {
      const cache = new LRU(10);
      const fib = cached(
        (n: number): number => {
          return n < 2 ? n : fib(n - 1) + fib(n - 2);
        },
        {cache},
      );
      expect(fib(20)).toBe(6765);
      expect(cache.get(List([20]))).toBe(6765);
    });

    it('provides access to cache via func.cache', () => {
      const fib = cached((n: number): number => {
        return n < 2 ? n : fib(n - 1) + fib(n - 2);
      });
      expect(fib(20)).toBe(6765);
      expect(fib.cache.get(List([20]))).toBe(6765);
    });

    it('counts cache miss and hit if cache.stats is present', () => {
      const fib = cached((n: number): number => {
        return n < 2 ? n : fib(n - 1) + fib(n - 2);
      });
      fib.cache.stats = {};
      expect(fib(20)).toBe(6765);
      expect(fib.cache.stats).toMatchObject({hit: 18, miss: 21});
    });

    it('skips cache if an arg is non-cachable', () => {
      const max = cached((a: number, b: number, map?: (v: number) => number): number => {
        const pickA = map == null ? a > b : map(a) > map(b);
        return pickA ? a : b;
      });
      const stats = (max.cache.stats = {});
      // number is cachable.
      expect(max(1, 2) + max(1, 2)).toBe(4);
      expect(stats).toMatchObject({hit: 1, miss: 1});
      // function is not cachable.
      expect(max(1, 2, v => 3 - v) + max(1, 2, v => 3 - v)).toBe(2);
      expect(max(1, 2, v => v)).toBe(2);
      expect(stats).toMatchObject({skip: 3});
    });

    it('can audit results', () => {
      let n = 0;
      const inc = cached(
        (rhs: number): number => {
          n += 1;
          return n + rhs;
        },
        {audit: true},
      );
      expect(inc(1)).toBe(2);
      expect(() => inc(1)).toThrow();
    });
  });

  describe('for class methods', () => {
    it('can be used as a decorator', () => {
      let calledTimes = 0;
      class Fib {
        @cached()
        fib(n: number): number {
          calledTimes += 1;
          return n < 2 ? n : this.fib(n - 1) + this.fib(n - 2);
        }
      }
      const f = new Fib();
      expect(f.fib(20)).toBe(6765);
      expect(calledTimes).toBe(21);
    });

    it('takes properties as extra cache keys', () => {
      const cache: LRUWithStats = new LRU(10);
      class Add {
        // lhs will be used as an extra cache key.
        lhs: number;
        constructor(lhs: number) {
          this.lhs = lhs;
        }
        @cached({cache})
        add(rhs: number): number {
          return this.lhs + rhs;
        }
      }
      const stats = (cache.stats = {});
      const a1 = new Add(100);
      const a2 = new Add(200);
      // `add(5)` for both a1 and a2. No cache hit since lhs is different.
      expect(a1.add(5)).toBe(105);
      expect(a2.add(5)).toBe(205);
      expect(stats).toMatchObject({miss: 2});
      // `a3.add(5)` gets a cache hit, since a3.lhs == a1.lhs.
      const a3 = new Add(200);
      expect(a3.add(5)).toBe(205);
      expect(stats).toMatchObject({hit: 1});
    });

    it('takes immutable object as an extra key', () => {
      const cache: LRUWithStats = new LRU(10);
      // Position is an immutable Record, and will be used as an extra cache key.
      class Position extends Record({x: 10, y: 20}) {
        @cached({cache})
        offset(dx: number, dy: number): [number, number] {
          return [this.x + dx, this.y + dy];
        }
      }
      const stats = (cache.stats = {});
      const p1 = new Position();
      const p2 = new Position({x: 30, y: 40});
      [...Array(3)].forEach(() => {
        expect(p1.offset(1, 2)).toMatchObject([11, 22]);
      });
      expect(stats).toMatchObject({miss: 1, hit: 2});
      [...Array(3)].forEach(() => {
        expect(p2.offset(1, 2)).toMatchObject([31, 42]);
      });
      expect(stats).toMatchObject({miss: 2, hit: 4});
      // p3 is a different instance, but can reuse p2 cache
      // since Immutable.is(p2, p3).
      const p3 = new Position({x: 30, y: 40});
      expect(p3).not.toBe(p1);
      expect(p3.offset(1, 2)).toMatchObject([31, 42]);
      expect(stats).toMatchObject({miss: 2, hit: 5});
    });

    it('can audit results', () => {
      let n = 0;
      class Impure {
        @cached({audit: true})
        inc(rhs: number): number {
          n += 1;
          return n + rhs;
        }
      }
      const obj = new Impure();
      expect(obj.inc(1)).toBe(2);
      expect(() => obj.inc(1)).toThrow();
    });

    it('can clear cache', () => {
      let fCalled = 0;
      let gCalled = 0;

      class A {
        @cached({track: true})
        f() {
          fCalled += 1;
          return 1;
        }
        @cached({track: false})
        g() {
          gCalled += 1;
          return 1;
        }
      }

      const a = new A();
      a.f();
      a.f();
      a.g();
      a.g();
      expect(fCalled).toBe(1);
      expect(gCalled).toBe(1);
      clearTrackedCache();
      a.f();
      a.g();
      expect(fCalled).toBe(2);
      expect(gCalled).toBe(1);
    });
  });
});
