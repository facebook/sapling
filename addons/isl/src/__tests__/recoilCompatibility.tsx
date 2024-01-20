/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {render} from '@testing-library/react';
import {List} from 'immutable';
import {RecoilRoot, atom, useRecoilValue} from 'recoil';
import {SelfUpdate} from 'shared/immutableExt';

class Foo extends SelfUpdate<List<number>> {}

const testFooAtom = atom<Foo>({
  key: 'testFooAtom',
  default: new Foo(List()),
});

describe('recoil compatibility', () => {
  it('does not freeze SelfUpdate types', () => {
    function MyTestComponent() {
      const foo = useRecoilValue(testFooAtom);
      expect(Object.isSealed(foo)).toBe(false);
      expect(Object.isFrozen(foo)).toBe(false);
      return null;
    }
    render(
      <RecoilRoot>
        <MyTestComponent />
      </RecoilRoot>,
    );
  });
});
