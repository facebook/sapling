/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {AccessGlobalRecoil} from '../AccessGlobalRecoil';
import {lazyAtom, readAtom, writeAtom} from '../jotaiUtils';
import {entangledAtoms} from '../recoilUtils';
import {render} from '@testing-library/react';
import {List} from 'immutable';
import {atom, useAtom, useAtomValue} from 'jotai';
import {useRef, useState, useEffect} from 'react';
import {RecoilRoot, atom as recoilAtom, useRecoilState, useRecoilValue} from 'recoil';
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
