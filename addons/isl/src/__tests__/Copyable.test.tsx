/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {render, screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {ViewportOverlayRoot} from 'isl-components/ViewportOverlay';
import {Copyable} from '../Copyable';
import platform from '../platform';

describe('Copyable', () => {
  beforeEach(() => {
    render(<ViewportOverlayRoot />);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('isolates keyboard activation from parent keyboard actions', () => {
    const copySpy = jest.spyOn(platform, 'clipboardCopy').mockImplementation(() => {});
    const parentKeyPress = jest.fn();
    render(
      <div data-testid="parent">
        <Copyable>copy me</Copyable>
      </div>,
    );
    screen.getByTestId('parent').addEventListener('keypress', parentKeyPress);

    screen.getByRole('button', {name: 'copy me'}).focus();
    userEvent.keyboard('{Enter}');

    expect(copySpy).toHaveBeenCalledWith('copy me');
    expect(parentKeyPress).not.toHaveBeenCalled();
  });
});
