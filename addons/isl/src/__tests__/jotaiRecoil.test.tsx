/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {AtomFamilyWeak} from '../jotaiUtils';
import type {Atom} from 'jotai';

import {AccessGlobalRecoil} from '../AccessGlobalRecoil';
import {atomFamilyWeak, lazyAtom, readAtom, writeAtom} from '../jotaiUtils';
import {entangledAtoms, jotaiMirrorFromRecoil} from '../recoilUtils';
import {render} from '@testing-library/react';
import {List} from 'immutable';
import {Provider, atom, createStore, useAtom, useAtomValue} from 'jotai';
import {type MeasureMemoryOptions, measureMemory} from 'node:vm';
import {useRef, useState, useEffect} from 'react';
import {act} from 'react-dom/test-utils';
import {
  RecoilRoot,
  atom as recoilAtom,
  selector,
  useRecoilState,
  useRecoilValue,
  useSetRecoilState,
} from 'recoil';
import {SelfUpdate} from 'shared/immutableExt';
import {nextTick} from 'shared/testUtils';

class Foo extends SelfUpdate<List<number>> {}

const testFooRecoilAtom = recoilAtom<Foo>({
  key: 'testFooAtom',
  default: new Foo(List()),
});

const testFooAtom = atom<Foo>(new Foo(List()));

describe('recoil compatibility', () => {
  it('does not freeze SelfUpdate types', () => {
    function MyTestComponent() {
      const foo = useRecoilValue(testFooRecoilAtom);
      expect(Object.isSealed(foo)).toBe(false);
      expect(Object.isFrozen(foo)).toBe(false);

      const foo2 = useAtomValue(testFooAtom);
      expect(Object.isSealed(foo2)).toBe(false);
      expect(Object.isFrozen(foo2)).toBe(false);

      return null;
    }
    render(
      <RecoilRoot>
        <MyTestComponent />
      </RecoilRoot>,
    );
  });
});

describe('entangledAtoms', () => {
  const [jotaiAtom, recoilAtom] = entangledAtoms({default: 'default', key: 'testEntangledAtom'});

  type TestProps = {
    update?: string;
    postUpdate?: () => void;
    readValue?: (value: string) => void;
  };

  function useTestHook(props: TestProps, [value, setValue]: [string, (value: string) => void]) {
    const updated = useRef(false);
    const {update, readValue, postUpdate} = props;
    useEffect(() => {
      if (update && !updated.current) {
        setValue(update);
        postUpdate?.();
        updated.current = true;
      }
    }, [update, setValue, postUpdate, updated]);
    if (readValue) {
      readValue(value);
    }
  }

  function Jotai(props: TestProps) {
    useTestHook(props, useAtom(jotaiAtom));
    return null;
  }

  function Recoil(props: TestProps) {
    useTestHook(props, useRecoilState(recoilAtom));
    return null;
  }

  [Recoil, Jotai].forEach(From => {
    [Recoil, Jotai].forEach(To => {
      it(`updates from ${From.name} to ${To.name}`, () => {
        const message = `${From.name}-to-${To.name}`;
        let readMessage: string | undefined = undefined;

        // The TestApp first uses `From` (could be either Jotai or Recoil)
        // to set the value. Once the value is set (updated === true),
        // renders `To` to read the value into `readMessage`.
        function TestApp() {
          const [updated, setUpdated] = useState(false);
          const handleReadValue = (v: string) => (readMessage = v);
          return (
            <RecoilRoot>
              <AccessGlobalRecoil />
              <From postUpdate={() => setUpdated(true)} update={message} />
              {updated && <To readValue={handleReadValue} />}
            </RecoilRoot>
          );
        }

        render(<TestApp />);

        expect(readMessage).toBe(message);
      });
    });
  });
});

describe('lazyAtom', () => {
  it('returns sync load() value', () => {
    const a = lazyAtom(() => 1, 2);
    expect(readAtom(a)).toBe(1);
  });

  it('returns fallback value and sets async load() value', async () => {
    const a = lazyAtom(() => Promise.resolve(1), 2);
    expect(readAtom(a)).toBe(2);
    await nextTick();
    expect(readAtom(a)).toBe(1);
  });

  it('can depend on another atom', () => {
    const a = atom<number | undefined>(undefined);
    const b = lazyAtom(() => 2, 2, a);

    expect(readAtom(a)).toBe(undefined);

    // Reading `b` triggers updating `a`.
    expect(readAtom(b)).toBe(2);
    expect(readAtom(a)).toBe(2);

    // If `a` is updated to be not `undefined`, `b` will be the same value.
    writeAtom(a, 3);
    expect(readAtom(b)).toBe(3);

    // Updating `b` updates `a` too.
    writeAtom(b, 4);
    expect(readAtom(a)).toBe(4);

    // If `a` is updated to be `undefined`, `b` will be the fallback value.
    writeAtom(a, undefined);
    expect(readAtom(b)).toBe(2);
  });
});

describe('jotaiMirrorFromRecoil', () => {
  it('mirrors Recoil atom value to Jotai atom', () => {
    const testRecoilMirrorAtom = recoilAtom<number>({
      key: 'testRecoilMirrorAtom',
      default: 10,
    });
    const testRecoilMirrorSelector = selector({
      key: 'testRecoilMirrorSelector',
      get: ({get}) => get(testRecoilMirrorAtom) * 2,
    });
    const jotaiAtom = jotaiMirrorFromRecoil(testRecoilMirrorSelector);

    let value = 0;

    function TestApp() {
      const setValue = useSetRecoilState(testRecoilMirrorAtom);
      value = useAtomValue(jotaiAtom);
      return <button onClick={() => setValue(v => v + 1)} className="inc" />;
    }

    render(
      <RecoilRoot>
        <AccessGlobalRecoil />
        <TestApp />
      </RecoilRoot>,
    );
    expect(value).toBe(20);

    act(() => {
      const button = document.querySelector('.inc') as HTMLButtonElement;
      button.click();
    });
    expect(value).toBe(22);
  });
});

describe('atomFamilyWeak', () => {
  let recalcFamilyCount = 0;
  let recalcAtomCount = 0;
  const family = atomFamilyWeak((key: number) => {
    recalcFamilyCount += 1;
    return atom(_get => {
      recalcAtomCount += 1;
      return key;
    });
  });

  let store = createStore();

  function TestComponent({k, f}: {k: number; f?: AtomFamilyWeak<number, Atom<number>>}) {
    const value = useAtomValue((f ?? family)(k));
    return <div>{value}</div>;
  }
  function TestApp({
    keys,
    family,
  }: {
    keys: Array<number>;
    family?: AtomFamilyWeak<number, Atom<number>>;
  }) {
    return (
      <Provider store={store}>
        {keys.map(k => (
          <TestComponent k={k} key={k} f={family} />
        ))}
      </Provider>
    );
  }

  beforeEach(() => {
    store = createStore();
    family.clear();
    recalcFamilyCount = 0;
    recalcAtomCount = 0;
  });

  async function gc() {
    // 'node --expose-gc' defines 'global.gc'.
    // To run with yarn: yarn node --expose-gc ./node_modules/.bin/jest ...
    const globalGc = global.gc;
    if (globalGc != null) {
      await new Promise<void>(r =>
        setTimeout(() => {
          globalGc();
          r();
        }, 0),
      );
    } else {
      // measureMemory with 'eager' has a side effect of running the GC.
      // This exists since node 14.
      // 'as' used since `MeasureMemoryOptions` is outdated (node 13?).
      await measureMemory({execution: 'eager'} as MeasureMemoryOptions);
    }
  }

  it('with "strong" cache enabled (default), unmount/remount skips re-calculate', async () => {
    const rendered = render(<TestApp keys={[1, 2, 3]} />);
    expect(recalcFamilyCount).toBe(3);
    expect(recalcAtomCount).toBe(3);

    rendered.rerender(<TestApp keys={[1, 2, 3]} />);
    expect(recalcFamilyCount).toBe(3);
    expect(recalcAtomCount).toBe(3);

    // After unmount, part of the atomFamily cache can still be used when re-mounting.
    rendered.unmount();
    await gc();

    render(<TestApp keys={[3, 2, 1]} />);
    expect(recalcFamilyCount).toBeLessThan(6);
    expect(recalcAtomCount).toBeLessThan(6);
  });

  it('with "strong" cache disabled, unmount/remount might re-calculate', async () => {
    let count = 0;
    const family = atomFamilyWeak(
      (key: number) => {
        count += 1;
        return atom(_get => key);
      },
      {useStrongCache: false},
    );

    const rendered = render(<TestApp keys={[1, 2, 3]} family={family} />);
    expect(count).toBe(3);

    rendered.unmount();
    await gc();

    render(<TestApp keys={[1, 2, 3]} family={family} />);
    expect(count).toBe(6);
  });

  it('"cleanup" does not clean in-use states', () => {
    const rendered = render(<TestApp keys={[1, 2, 3]} />);
    expect(recalcFamilyCount).toBe(3);
    expect(recalcAtomCount).toBe(3);
    family.cleanup();

    // re-render can still use cached state after "cleanup" (count remains the same).
    rendered.rerender(<TestApp keys={[1, 2, 3]} />);
    expect(recalcFamilyCount).toBe(3);
    expect(recalcAtomCount).toBe(3);
  });

  it('"cleanup" can release memory', async () => {
    const rendered = render(<TestApp keys={[1, 2, 3]} />);
    expect(recalcFamilyCount).toBe(3);
    expect(recalcAtomCount).toBe(3);
    rendered.unmount();

    await gc();
    family.cleanup();
    await gc();
    family.cleanup();

    // umount, then re-render will recalculate all atoms (count increases).
    render(<TestApp keys={[3, 2, 1]} />);
    expect(recalcFamilyCount).toBe(6);
    expect(recalcAtomCount).toBe(6);
  });

  it('"clear" releases memory', () => {
    const rendered = render(<TestApp keys={[1, 2, 3]} />);
    expect(recalcFamilyCount).toBe(3);
    expect(recalcAtomCount).toBe(3);
    family.clear();

    // re-render will recalculate all atoms.
    rendered.rerender(<TestApp keys={[1, 2, 3]} />);
    expect(recalcFamilyCount).toBe(6);
    expect(recalcAtomCount).toBe(6);
    rendered.unmount();
    family.clear();

    // umount, then re-render will recalculate all atoms (count increases).
    render(<TestApp keys={[3, 2, 1]} />);
    expect(recalcFamilyCount).toBe(9);
    expect(recalcAtomCount).toBe(9);
  });

  it('"cleanup" runs automatically to reduce cache size', async () => {
    const N = 10;
    const M = 30;

    // Render N items.
    const rendered = render(<TestApp keys={Array.from({length: N}, (_, i) => i)} />);

    // Umount to drop references to the atoms.
    rendered.unmount();

    // Force GC to run to stabilize the test.
    await gc();

    // After GC, render M items with different keys.
    // This would trigger `family.cleanup` transparently.
    render(<TestApp keys={Array.from({length: M}, (_, i) => N + i)} />);

    // Neither of the caches should have N + M items (which means no cleanup).
    expect(family.weakCache.size).toBeLessThan(N + M);
    expect(family.strongCache.size).toBeLessThan(N + M);
  });

  it('provides debugLabel', () => {
    const family = atomFamilyWeak((v: string) => atom(v));
    family.debugLabel = 'prefix1';
    const atom1 = family('a');
    expect(atom1.debugLabel).toBe('prefix1:a');
  });
});
