/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import type {RenderResult} from '@testing-library/react';

import {act, render, screen} from '@testing-library/react';
import {Delayed} from '../Delayed';

describe('<Delayed />', () => {
  let rendered: RenderResult | null = null;

  const renderDelayed = (hideUntil: Date) => {
    rendered = render(
      <Delayed hideUntil={hideUntil}>
        <span>inner</span>
      </Delayed>,
    );
  };

  beforeEach(() => {
    jest.useFakeTimers();
  });

  afterEach(() => {
    if (rendered != null) {
      rendered.unmount();
      rendered = null;
    }
    jest.useRealTimers();
  });

  it('hides children with a future "hideUntil"', () => {
    const future = new Date(Date.now() + 1);
    renderDelayed(future);
    expect(screen.queryByText('inner')).toBeNull();
  });

  it('shows children with a past "hideUntil"', () => {
    const past = new Date(Date.now() - 1);
    renderDelayed(past);
    expect(screen.queryByText('inner')).toBeInTheDocument();
  });

  it('hides then shows children with a future "hideUntil"', () => {
    const future = new Date(Date.now() + 1);
    renderDelayed(future);
    expect(screen.queryByText('inner')).toBeNull();
    act(() => {
      jest.advanceTimersByTime(1);
    });
    expect(screen.queryByText('inner')).toBeInTheDocument();
  });
});
