/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @jest-environment jsdom
 */

import {makeCommandDispatcher, KeyCode, Modifier} from '../KeyboardShortcuts';
import {render, screen} from '@testing-library/react';
import '@testing-library/jest-dom';
import userEvent from '@testing-library/user-event';
import {useCallback, useState} from 'react';
import {act} from 'react-dom/test-utils';

/* eslint-disable no-bitwise */

describe('KeyboardShortcuts', () => {
  it('handles callbacks in components', () => {
    const [ShortcutContext, useCommand] = makeCommandDispatcher({
      foo: [Modifier.SHIFT | Modifier.CMD | Modifier.CTRL, KeyCode.One],
    });

    function MyComponent() {
      const [value, setValue] = useState('bad');
      const onFoo = useCallback(() => {
        setValue('good');
      }, [setValue]);
      useCommand('foo', onFoo);
      return <div>{value}</div>;
    }

    render(
      <ShortcutContext>
        <MyComponent />
      </ShortcutContext>,
    );

    expect(screen.queryByText('good')).not.toBeInTheDocument();
    act(() => {
      userEvent.type(document.body, '{shift}{meta}{ctrl}1');
    });
    expect(screen.queryByText('good')).toBeInTheDocument();
  });

  it('only triggers exactly the command requested', () => {
    const [ShortcutContext, useCommand] = makeCommandDispatcher({
      expected: [Modifier.CTRL | Modifier.CMD, KeyCode.Two],
      bad0: [Modifier.NONE, KeyCode.Two],
      bad1: [Modifier.CTRL, KeyCode.Two],
      bad2: [Modifier.CMD, KeyCode.Two],
      bad3: [Modifier.SHIFT, KeyCode.Two],
      bad4: [Modifier.ALT, KeyCode.Two],
      bad5: [Modifier.CTRL | Modifier.CMD, KeyCode.Three],
      bad6: [Modifier.ALT | Modifier.SHIFT, KeyCode.Two],
      bad7: [Modifier.SHIFT | Modifier.CMD | Modifier.CTRL, KeyCode.Two],
      bad8: [Modifier.ALT | Modifier.CMD | Modifier.CTRL, KeyCode.Two],
    });

    function MyComponent() {
      const [value, setValue] = useState('bad');
      const onFoo = useCallback(() => {
        setValue('good');
      }, [setValue]);
      const die = () => {
        throw new Error('wrong command triggered');
      };
      useCommand('expected', onFoo);
      useCommand('bad0', die);
      useCommand('bad1', die);
      useCommand('bad2', die);
      useCommand('bad3', die);
      useCommand('bad4', die);
      useCommand('bad5', die);
      useCommand('bad6', die);
      useCommand('bad7', die);
      useCommand('bad8', die);
      return <div>{value}</div>;
    }

    render(
      <ShortcutContext>
        <MyComponent />
      </ShortcutContext>,
    );

    expect(screen.queryByText('good')).not.toBeInTheDocument();
    act(() => {
      userEvent.type(document.body, '{ctrl}{meta}2');
    });
    expect(screen.queryByText('good')).toBeInTheDocument();
  });

  it("typing into input doesn't trigger commands", () => {
    const [ShortcutContext, useCommand] = makeCommandDispatcher({
      foo: [Modifier.SHIFT, KeyCode.D],
    });

    let value = 0;
    function MyComponent() {
      useCommand('foo', () => {
        value++;
      });
      return (
        <div>
          {value}
          <textarea data-testid="myTextArea" />
          <input data-testid="myInput" />
        </div>
      );
    }

    render(
      <ShortcutContext>
        <MyComponent />
      </ShortcutContext>,
    );

    expect(value).toEqual(0);
    act(() => {
      userEvent.type(document.body, '{shift}D');
    });
    expect(value).toEqual(1);
    act(() => {
      userEvent.type(screen.getByTestId('myTextArea'), '{shift}D');
    });
    expect(value).toEqual(1);
    act(() => {
      userEvent.type(screen.getByTestId('myInput'), '{shift}D');
    });
    expect(value).toEqual(1);
  });

  it('only subscribes to component listeners while mounted', () => {
    const [ShortcutContext, useCommand] = makeCommandDispatcher({
      bar: [Modifier.CMD, KeyCode.Four],
    });

    let value = 0;
    let savedSetRenderChild: undefined | ((value: boolean) => unknown) = undefined;
    function MyWrapper() {
      const [renderChild, setRenderChild] = useState(true);
      savedSetRenderChild = setRenderChild;
      if (renderChild) {
        return <MyComponent />;
      }
      return null;
    }

    function MyComponent() {
      const onBar = useCallback(() => {
        value++;
      }, []);
      useCommand('bar', onBar);
      return <div>{value}</div>;
    }

    render(
      <ShortcutContext>
        <MyWrapper />
      </ShortcutContext>,
    );

    expect(value).toEqual(0);
    act(() => {
      userEvent.type(document.body, '{meta}4');
    });
    expect(value).toEqual(1);

    // unmount
    act(() => {
      (savedSetRenderChild as unknown as (value: boolean) => unknown)(false);
    });

    act(() => {
      userEvent.type(document.body, '{meta}4');
    });
    // shortcut no longer does anything
    expect(value).toEqual(1);
  });

  it('handles multiple subscribers', () => {
    let value = 0;
    const [ShortcutContext, useCommand] = makeCommandDispatcher({
      boz: [Modifier.CMD, KeyCode.Five],
    });

    function MyComponent() {
      const onBoz = useCallback(() => {
        value++;
      }, []);
      useCommand('boz', onBoz);
      return <div>{value}</div>;
    }

    render(
      <ShortcutContext>
        <MyComponent />
        <MyComponent />
      </ShortcutContext>,
    );

    expect(value).toBe(0);
    act(() => {
      userEvent.type(document.body, '{meta}5');
    });
    expect(value).toBe(2);
  });
});
