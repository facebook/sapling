/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {AccessGlobalRecoil} from '../AccessGlobalRecoil';
import {entangledAtoms} from '../recoilUtils';
import {render} from '@testing-library/react';
import {List} from 'immutable';
import {atom, useAtom, useAtomValue} from 'jotai';
import {useRef, useState, useEffect} from 'react';
import {RecoilRoot, atom as recoilAtom, useRecoilState, useRecoilValue} from 'recoil';
import {SelfUpdate} from 'shared/immutableExt';

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
  const originalAtom = atom<string>('default');
  const [jotaiAtom, recoilAtom] = entangledAtoms(originalAtom, 'testEntangledAtom');

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
