/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {AccessGlobalRecoil, globalRecoil} from '../AccessGlobalRecoil';
import {act, fireEvent, render, screen} from '@testing-library/react';
import {atom, DefaultValue, RecoilRoot, selector, useRecoilState, useRecoilValue} from 'recoil';

describe('AccessGlobalRecoil', () => {
  it('allows getting atoms', () => {
    const myAtom = atom({
      key: 'myAtom',
      default: 0,
    });
    const MyComponent = () => {
      const [val, setVal] = useRecoilState(myAtom);
      return <div onClick={() => setVal(42)}>{val}</div>;
    };
    const App = () => (
      <div>
        <RecoilRoot>
          <AccessGlobalRecoil name="test1" />
          <MyComponent />
        </RecoilRoot>
      </div>
    );

    render(<App />);

    fireEvent.click(screen.getByText('0'));

    expect(globalRecoil('test1').getLoadable(myAtom).valueOrThrow()).toEqual(42);
  });

  it('allows setting atoms', () => {
    const myAtom2 = atom({
      key: 'myAtom2',
      default: 0,
    });
    const MyComponent = () => {
      const val = useRecoilValue(myAtom2);
      return <div>{val}</div>;
    };
    const App = () => (
      <div>
        <RecoilRoot>
          <AccessGlobalRecoil name="test2" />
          <MyComponent />
        </RecoilRoot>
      </div>
    );

    render(<App />);

    act(() => globalRecoil('test2').set(myAtom2, 42));
    expect(screen.getByText('42')).toBeInTheDocument();
  });

  it('allows setting selectors', () => {
    const myUnderlyingState = atom({
      key: 'myUnderlyingState',
      default: 0,
    });
    const mySelector = selector({
      key: 'mySelector',
      get: ({get}) => get(myUnderlyingState) + 1,
      set: ({set}, newValue) => {
        set(myUnderlyingState, newValue instanceof DefaultValue ? -1 : newValue - 1);
      },
    });
    const MyComponent = () => {
      const val = useRecoilValue(mySelector);
      return <div>{val}</div>;
    };
    const App = () => (
      <div>
        <RecoilRoot>
          <AccessGlobalRecoil name="test2" />
          <MyComponent />
        </RecoilRoot>
      </div>
    );

    render(<App />);

    act(() => globalRecoil('test2').set(mySelector, 42));
    expect(screen.getByText('42')).toBeInTheDocument();
  });
});
