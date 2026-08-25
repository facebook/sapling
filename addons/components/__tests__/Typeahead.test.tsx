/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @jest-environment jsdom
 */

import type {TypeaheadResult} from '../Types';

import '@testing-library/jest-dom';
import {act, fireEvent, render, screen} from '@testing-library/react';
import {useState} from 'react';
import {Typeahead} from '../Typeahead';

function makeFetchTokens(values: Array<TypeaheadResult> = []) {
  // `fetchStartTimestamp` is stamped when the fetch starts, matching `fetchNewSuggestions`. Fake
  // timers fake `Date` too, so this moves with `advanceTimersByTime`.
  return jest.fn((_prefix: string) => Promise.resolve({values, fetchStartTimestamp: Date.now()}));
}

/**
 * Like {@link makeFetchTokens}, but each fetch stays pending until its entry in `pending` is
 * settled, so several can be in flight at once and resolve out of the order they started in.
 */
function makeDeferredFetchTokens() {
  const pending: Array<{
    timestamp: number;
    settle: (values: Array<TypeaheadResult>) => void;
  }> = [];
  const fetchTokens = jest.fn((_prefix: string) => {
    const fetchStartTimestamp = Date.now();
    return new Promise<{values: Array<TypeaheadResult>; fetchStartTimestamp: number}>(resolve => {
      pending.push({
        timestamp: fetchStartTimestamp,
        settle: values => resolve({values, fetchStartTimestamp}),
      });
    });
  });
  return {fetchTokens, pending};
}

function suggestion(value: string): TypeaheadResult {
  return {label: value, value};
}

/**
 * Accept a suggestion by clicking it. Clicking names the row the user is looking at, so it stays
 * available while the list is held over a query it was not fetched for; Enter, which acts on an
 * offscreen highlight, does not. Tests about accepting rather than about Enter use this.
 */
function acceptByClick(label: string) {
  act(() => {
    fireEvent.mouseDown(screen.getByText(label));
  });
}

/**
 * Mimics how Typeahead is actually used: `tokenString` lives in the caller's state, so every
 * keystroke re-renders the caller, and `fetchTokens` is a brand new function on each of those
 * renders (call sites use inline arrows or `.bind()`). The debouncer has to survive that churn,
 * otherwise it is rebuilt per keystroke and each keystroke schedules its own fetch.
 */
function TypeaheadHarness({
  fetchTokens,
  debounceInterval,
}: {
  fetchTokens: (prefix: string) => Promise<{
    values: Array<TypeaheadResult>;
    fetchStartTimestamp: number;
  }>;
  debounceInterval?: number;
}) {
  const [tokenString, setTokenString] = useState('');
  return (
    <Typeahead
      tokenString={tokenString}
      setTokenString={setTokenString}
      fetchTokens={prefix => fetchTokens(prefix)}
      autoFocus={false}
      debounceInterval={debounceInterval}
      data-testid="typeahead-input"
    />
  );
}

/** Type `text` one character at a time, waiting `msBetweenKeystrokes` after each. */
function type(text: string, msBetweenKeystrokes: number) {
  const input = screen.getByTestId('typeahead-input');
  for (let i = 1; i <= text.length; i++) {
    act(() => {
      fireEvent.input(input, {target: {value: text.slice(0, i)}});
      jest.advanceTimersByTime(msBetweenKeystrokes);
    });
  }
}

/** Replace the whole input value in one event, rather than typing character by character. */
function setInput(value: string) {
  act(() => {
    fireEvent.input(screen.getByTestId('typeahead-input'), {target: {value}});
  });
}

/** Advance fake timers, letting the promise from any fetch that fires settle inside `act`. */
async function wait(ms: number) {
  await act(async () => {
    jest.advanceTimersByTime(ms);
    await Promise.resolve();
  });
}

/** Let the `.then` of a just-settled fetch run inside `act`. */
async function flush() {
  await act(async () => {
    await Promise.resolve();
  });
}

function loadingSpinner(): HTMLElement | null {
  return document.querySelector('.codicon-loading');
}

describe('Typeahead', () => {
  beforeEach(() => {
    jest.useFakeTimers();
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  it('debounces without the call site opting in', async () => {
    const fetchTokens = makeFetchTokens();
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    type('abcdefghij', 50);
    // 500ms of typing, but only 50ms since the last keystroke
    expect(fetchTokens).not.toHaveBeenCalled();

    // Pin the default to exactly 300ms, not just "somewhere under 350".
    await wait(249);
    expect(fetchTokens).not.toHaveBeenCalled();

    await wait(1);
    expect(fetchTokens).toHaveBeenCalledTimes(1);
    expect(fetchTokens).toHaveBeenCalledWith('abcdefghij');
  });

  it('respects a caller-provided debounceInterval', async () => {
    const fetchTokens = makeFetchTokens();
    render(<TypeaheadHarness fetchTokens={fetchTokens} debounceInterval={1000} />);

    type('abc', 0);
    await wait(999);
    expect(fetchTokens).not.toHaveBeenCalled();

    await wait(1);
    expect(fetchTokens).toHaveBeenCalledTimes(1);
    expect(fetchTokens).toHaveBeenCalledWith('abc');
  });

  it('drops a pending fetch when unmounted', async () => {
    const fetchTokens = makeFetchTokens();
    const {unmount} = render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    type('abc', 0);
    unmount();

    await wait(1000);
    expect(fetchTokens).not.toHaveBeenCalled();
  });

  it('fetches with the latest fetchTokens, not the one captured on first render', async () => {
    const first = makeFetchTokens();
    const second = makeFetchTokens();
    const {rerender} = render(<TypeaheadHarness fetchTokens={first} />);

    type('abc', 0);
    rerender(<TypeaheadHarness fetchTokens={second} />);

    await wait(300);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledWith('abc');
  });

  it('drops a pending fetch when a suggestion is accepted', async () => {
    const fetchTokens = makeFetchTokens([suggestion('alice'), suggestion('alicia')]);
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    // Let one fetch land so the dropdown is showing suggestions.
    type('ali', 0);
    await wait(300);
    expect(screen.getByText('alicia')).toBeInTheDocument();

    // One more character schedules a second fetch. The dropdown stays up meanwhile, showing the
    // values from the first fetch, so it is still possible to accept one. `setInput` rather than
    // `type`, which replays from the first character and would walk the query back to `a` --
    // no longer an extension of `ali`, so the list would be dropped and there would be nothing
    // left to accept.
    setInput('alic');
    acceptByClick('alice');

    // Without the `reset()` a second fetch goes out for text the accept has already consumed. Only
    // the call count catches that: the reply itself would be turned away by the reply-time guard,
    // because accepting cleared the live query.
    await wait(1000);
    expect(fetchTokens).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('alicia')).not.toBeInTheDocument();
  });

  it('shows the loading spinner for the whole pending window', async () => {
    const {fetchTokens, pending} = makeDeferredFetchTokens();
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    // The spinner goes up on the keystroke, not when the fetch starts: moving it to the fetch
    // would leave every call site blank for the whole debounce interval.
    setInput('ali');
    expect(loadingSpinner()).toBeInTheDocument();

    await wait(299);
    expect(loadingSpinner()).toBeInTheDocument();
    expect(fetchTokens).not.toHaveBeenCalled();

    await wait(1);
    expect(fetchTokens).toHaveBeenCalledTimes(1);
    expect(loadingSpinner()).toBeInTheDocument();

    pending[0].settle([suggestion('alice')]);
    await flush();
    expect(loadingSpinner()).not.toBeInTheDocument();
    expect(screen.getByText('alice')).toBeInTheDocument();
  });

  it('ignores a slow fetch that resolves after a newer one', async () => {
    const {fetchTokens, pending} = makeDeferredFetchTokens();
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    setInput('al');
    await wait(300);
    setInput('ali');
    await wait(300);
    expect(fetchTokens).toHaveBeenCalledTimes(2);
    expect(pending[1].timestamp).toBeGreaterThan(pending[0].timestamp);

    // The newer fetch lands first...
    pending[1].settle([suggestion('alicia')]);
    await flush();
    expect(screen.getByText('alicia')).toBeInTheDocument();

    // ...so the older one, arriving late, must not overwrite it with results for a shorter prefix.
    pending[0].settle([suggestion('alan')]);
    await flush();
    expect(screen.getByText('alicia')).toBeInTheDocument();
    expect(screen.queryByText('alan')).not.toBeInTheDocument();
  });

  it('keeps the dropdown closed when a suggestion is accepted in the same millisecond a fetch started', async () => {
    const {fetchTokens, pending} = makeDeferredFetchTokens();
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    // Land one fetch so there is something to accept.
    setInput('ali');
    await wait(300);
    pending[0].settle([suggestion('alice'), suggestion('alicia')]);
    await flush();
    expect(screen.getByText('alicia')).toBeInTheDocument();

    // A second fetch *starts*. `reset()` can no longer cancel it: the debouncer clears its timer
    // before invoking the callback, so accepting a suggestion now cannot call it off.
    setInput('alic');
    await wait(300);
    expect(fetchTokens).toHaveBeenCalledTimes(2);

    // The accept clears the live query, and that is what turns this reply away — the timestamp
    // comparison never runs. Pinning the same millisecond is still deliberate: it is the case a
    // timestamp-only guard would get wrong, since `last.timestamp > fetchStartTimestamp` is strict.
    // Fake timers freeze `Date.now()` between advances, so the equality below is exact.
    expect(Date.now()).toBe(pending[1].timestamp);
    acceptByClick('alice');

    pending[1].settle([suggestion('alicia')]);
    await flush();
    expect(screen.queryByText('alicia')).not.toBeInTheDocument();
  });

  it('keeps the dropdown closed when the accept lands a millisecond later', async () => {
    const {fetchTokens, pending} = makeDeferredFetchTokens();
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    setInput('ali');
    await wait(300);
    pending[0].settle([suggestion('alice'), suggestion('alicia')]);
    await flush();
    // Assert the precondition, so the negative assertion below cannot pass by the dropdown never
    // having opened at all.
    expect(screen.getByText('alicia')).toBeInTheDocument();

    setInput('alic');
    await wait(300);

    // The control for the other direction: a millisecond of daylight, where even a timestamp-only
    // guard would be right. Together the two pin the behaviour whichever guard implements it.
    await wait(1);
    expect(Date.now()).toBeGreaterThan(pending[1].timestamp);
    acceptByClick('alice');

    pending[1].settle([suggestion('alicia')]);
    await flush();
    expect(screen.queryByText('alicia')).not.toBeInTheDocument();
  });

  it('drops the suggestion list once the query stops extending it', async () => {
    const byPrefix: {[prefix: string]: Array<TypeaheadResult>} = {
      ali: [suggestion('alice'), suggestion('alicia')],
      bob: [suggestion('bobby')],
    };
    const fetchTokens = jest.fn((prefix: string) =>
      Promise.resolve({values: byPrefix[prefix] ?? [], fetchStartTimestamp: Date.now()}),
    );
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    // Land a list for `ali`.
    setInput('ali');
    await wait(300);
    expect(screen.getByText('alicia')).toBeInTheDocument();

    // Clear the field and type something unrelated, all inside one debounce window. Nothing in the
    // `ali` list matches `bob`, and the refetch that would replace it is a full interval away, so a
    // list kept here stays acceptable by Enter that entire time.
    setInput('');
    setInput('bob');
    expect(screen.queryByText('alicia')).not.toBeInTheDocument();
    expect(fetchTokens).toHaveBeenCalledTimes(1);

    act(() => {
      fireEvent.keyDown(screen.getByTestId('typeahead-input'), {key: 'Enter'});
    });

    // Accepting `alice` here would both commit the wrong token and discard what was typed, since
    // `saveNewValue` re-renders the field with an empty remainder.
    expect(screen.getByTestId('typeahead-input')).toHaveValue('bob');
    expect(screen.queryByText('alice')).not.toBeInTheDocument();
  });

  it('refuses Enter while the visible list was fetched for different text', async () => {
    const byPrefix: {[prefix: string]: Array<TypeaheadResult>} = {
      al: [suggestion('alan')],
      alic: [suggestion('alice')],
    };
    const fetchTokens = jest.fn((prefix: string) =>
      Promise.resolve({values: byPrefix[prefix] ?? [], fetchStartTimestamp: Date.now()}),
    );
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    setInput('al');
    await wait(300);
    expect(screen.getByText('alan')).toBeInTheDocument();

    // `alic` still extends `al`, so the list is deliberately held over — but it now describes text
    // the user has typed past. Enter here used to commit `alan` and throw away the `ic`.
    setInput('alic');
    expect(screen.getByText('alan')).toBeInTheDocument();
    act(() => {
      fireEvent.keyDown(screen.getByTestId('typeahead-input'), {key: 'Enter'});
    });
    expect(screen.getByTestId('typeahead-input')).toHaveValue('alic');

    // Once the list for `alic` lands, Enter means something again.
    await wait(300);
    act(() => {
      fireEvent.keyDown(screen.getByTestId('typeahead-input'), {key: 'Enter'});
    });
    expect(screen.getByText('alice')).toBeInTheDocument();
  });

  it('keeps the list, and the arrow keys, alive while a character is deleted', async () => {
    const fetchTokens = makeFetchTokens([suggestion('alice'), suggestion('alicia')]);
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    setInput('alicee');
    await wait(300);
    expect(screen.getByText('alicia')).toBeInTheDocument();

    // Backspacing a typo leaves the landed list incomplete rather than wrong — everything in it
    // still matches. Blanking it would take ArrowDown/ArrowUp/Enter down with it for the whole
    // debounce interval, since the key handler reads the values off the suggestion object.
    setInput('alice');
    expect(screen.getByText('alicia')).toBeInTheDocument();
    expect(loadingSpinner()).not.toBeInTheDocument();
  });

  it('keeps the suggestion list while the query is only extended', async () => {
    const fetchTokens = makeFetchTokens([suggestion('alice'), suggestion('alicia')]);
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    setInput('ali');
    await wait(300);
    expect(screen.getByText('alicia')).toBeInTheDocument();

    // The control for the test above: `alic` still extends the query these were fetched for, so
    // the list stays up. Blanking it on every keystroke would flash the spinner through the whole
    // debounce window.
    setInput('alic');
    expect(screen.getByText('alicia')).toBeInTheDocument();
    expect(loadingSpinner()).not.toBeInTheDocument();
  });

  it('drops a landed suggestion list when the field moved on while it was in flight', async () => {
    const {fetchTokens, pending} = makeDeferredFetchTokens();
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    // A fetch for `ali` gets under way...
    setInput('ali');
    await wait(300);
    expect(fetchTokens).toHaveBeenCalledTimes(1);

    // ...and the field is emptied and retyped before it answers. The `onInput` check cannot cover
    // this: the list arrives after the last keystroke, so no keystroke is left to evaluate it.
    setInput('');
    setInput('bob');
    pending[0].settle([suggestion('alice'), suggestion('alicia')]);
    await flush();
    expect(screen.queryByText('alicia')).not.toBeInTheDocument();

    act(() => {
      fireEvent.keyDown(screen.getByTestId('typeahead-input'), {key: 'Enter'});
    });
    expect(screen.getByTestId('typeahead-input')).toHaveValue('bob');
  });

  it('keeps a landed suggestion list when the field only extended while it was in flight', async () => {
    const {fetchTokens, pending} = makeDeferredFetchTokens();
    render(<TypeaheadHarness fetchTokens={fetchTokens} />);

    setInput('ali');
    await wait(300);

    // The control for the test above: `alic` still extends the query this fetch went out for, so a
    // reply arriving a beat late is still the best answer there is and has to be shown.
    setInput('alic');
    pending[0].settle([suggestion('alice'), suggestion('alicia')]);
    await flush();
    expect(screen.getByText('alicia')).toBeInTheDocument();
  });
});
