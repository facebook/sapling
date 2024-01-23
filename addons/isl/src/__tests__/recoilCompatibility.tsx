/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {render} from '@testing-library/react';
import {List} from 'immutable';
import {atom, useAtomValue} from 'jotai';
import {RecoilRoot, atom as recoilAtom, useRecoilValue} from 'recoil';
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
