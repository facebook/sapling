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
import {hideToast} from '../toast';
import {TopLevelToast} from '../TopLevelToast';

describe('Copyable', () => {
  beforeEach(() => {
    hideToast(['copied']);
    render(
      <>
        <ViewportOverlayRoot />
        <TopLevelToast />
      </>,
    );
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('shows success after keyboard copy and isolates it from parent actions', async () => {
    const copySpy = jest.spyOn(platform, 'clipboardCopy').mockResolvedValue();
    const parentKeyPress = jest.fn();
    render(
      <div data-testid="parent">
        <Copyable>copy me</Copyable>
      </div>,
    );
    screen.getByTestId('parent').addEventListener('keypress', parentKeyPress);

    screen.getByRole('button', {name: 'copy me'}).focus();
    userEvent.keyboard('{Enter}');

    expect(await screen.findByText('Copied copy me')).toBeInTheDocument();
    expect(copySpy).toHaveBeenCalledWith('copy me');
    expect(parentKeyPress).not.toHaveBeenCalled();
  });

  it('shows failure instead of success when clipboard access is rejected', async () => {
    jest
      .spyOn(platform, 'clipboardCopy')
      .mockRejectedValue(new DOMException('denied', 'NotAllowedError'));
    render(<Copyable>copy me</Copyable>);

    userEvent.click(screen.getByRole('button', {name: 'copy me'}));

    expect(await screen.findByText('Could not copy copy me')).toBeInTheDocument();
    expect(screen.queryByText('Copied copy me')).not.toBeInTheDocument();
  });
});
