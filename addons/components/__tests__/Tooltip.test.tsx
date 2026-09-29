/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @jest-environment jsdom
 */

import {fireEvent, render, screen} from '@testing-library/react';
import {Tooltip} from '../Tooltip';
import {ViewportOverlayRoot} from '../ViewportOverlay';

describe('Tooltip', () => {
  it('keeps the arrow outside the scrollable content', () => {
    render(
      <>
        <Tooltip title="Tooltip text">
          <button>Trigger</button>
        </Tooltip>
        <ViewportOverlayRoot />
      </>,
    );

    fireEvent.mouseEnter(screen.getByRole('button').parentElement as HTMLElement);

    const tooltip = screen.getByRole('tooltip');
    const content = tooltip.querySelector('.tooltip-content');
    const arrow = tooltip.querySelector('.tooltip-arrow');

    expect(tooltip).not.toHaveStyle({overflowY: 'auto'});
    expect(content).toHaveStyle({overflowY: 'auto'});
    expect(content?.parentElement).toBe(tooltip);
    expect(arrow?.parentElement).toBe(tooltip);
  });
});
