/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {Button} from 'isl-components/Button';
import {Checkbox} from 'isl-components/Checkbox';
import {DatetimePicker} from 'isl-components/DatetimePicker';
import {TextField} from 'isl-components/TextField';
import {useEffect, useRef, useState} from 'react';
import {tracker} from './analytics';
import {t, T} from './i18n';
import {GotoOperation} from './operations/GotoOperation';
import {RebaseOperation} from './operations/RebaseOperation';
import {useRunOperation} from './operationsState';
import {type ExactRevset, exactRevset} from './types';

import './GotoTimeMenu.css';

/**
 * Generates a succeedable revset for a specific number of hours ago
 * @param hours Number of hours ago
 * @returns A ExactRevset object
 */
function getRevsetForHoursAgo(hours: number): ExactRevset {
  const date = new Date();
  // setHours() correctly handles going back a day/month/year as needed, if it would take us from, eg, Jan 1st to Dec 31st
  date.setHours(date.getHours() - hours);
  const datetimeStr = formatDateTimeHelper(date);
  return getRevsetForDate(datetimeStr);
}

/**
 * Generates a succeedable revset for a specific date string
 * @param dateString The date string to use in the revset
 * @returns A ExactRevset object
 */
function getRevsetForDate(dateString: string): ExactRevset {
  return exactRevset(`bsearch(date(">${dateString}"),max(public()))`);
}

/**
 * Formats a date into a string compatible with the datetime-local input and SL revset.date()
 * @param date The date to format
 * @returns A string in the format YYYY-MM-DDTHH:MM
 */
function formatDateTimeHelper(date: Date): string {
  // Date.toISOString() is close to what we want, but it has fractional seconds and isn't in local time
  // Date.toLocaleString() is in local time, but uses slashes rather than dashes
  // Format date as YYYY-MM-DD in local timezone
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0'); // getMonth() is 0-indexed
  const day = String(date.getDate()).padStart(2, '0');

  // Format time as HH:MM in local timezone
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');

  // Return in format YYYY-MM-DDTHH:MM (required by datetime-local input and compatible with revset.date())
  return `${year}-${month}-${day}T${hours}:${minutes}`;
}

/**
 * GotoTimeContent component that can be used directly or wrapped in an expander
 */
export function GotoTimeContent({dismiss}: {dismiss?: () => unknown}) {
  const runOperation = useRunOperation();
  const [shouldRebase, setShouldRebase] = useState(false);
  const [hours, setHours] = useState('');
  const [datetime, setDatetime] = useState('');
  const maxDatetime = useRef('');
  const hoursInputRef = useRef(null);

  useEffect(() => {
    if (hoursInputRef.current) {
      (hoursInputRef.current as HTMLInputElement).focus();
    }

    // Initialize datetime and maxDatetime with current time.
    const now = new Date();
    const nowFormatted = formatDateTimeHelper(now);
    setDatetime(nowFormatted);
    maxDatetime.current = nowFormatted;
  }, [hoursInputRef]);

  // When hours is edited, clear the datetime picker. Must be one or the other
  const handleHoursChange = (value: string) => {
    setHours(value);
    if (value.trim().length > 0) {
      setDatetime('');
    }
  };

  // When datetime is edited, clear the hours input. Must be one or the other
  const handleDatetimeChange = (value: string) => {
    setDatetime(value);
    if (value.trim().length > 0) {
      setHours('');
    }
  };

  const doGoToCommit = () => {
    tracker.track('ClickGotoTimeButton', {
      extras: {
        isHours: hours.trim().length > 0,
        shouldRebase,
      },
    });

    // Get the destination revset based on "hours ago" or datetime, whichever has a value
    let destinationRevset: ExactRevset;

    if (hours.trim().length > 0) {
      const hoursValue = parseFloat(hours);
      if (isNaN(hoursValue)) {
        return;
      }
      destinationRevset = getRevsetForHoursAgo(hoursValue);
    } else if (datetime.trim().length > 0) {
      destinationRevset = getRevsetForDate(datetime);
    } else {
      // No valid input
      return;
    }

    if (shouldRebase) {
      // Rebase current work onto the commit at the specified time
      runOperation(new RebaseOperation(exactRevset('.'), destinationRevset));
    } else {
      // Go to the commit at the specified time
      runOperation(new GotoOperation(destinationRevset));
    }

    // Dismiss the tooltip/dialog if it exists
    if (dismiss) {
      dismiss();
    }
  };

  return (
    <div className="goto-time-content">
      <div className="goto-time-input-row">
        <TextField
          width="100%"
          placeholder={t('Hours ago')}
          value={hours}
          data-testid="goto-time-input"
          onInput={e => handleHoursChange((e.target as unknown as {value: string})?.value ?? '')}
          onKeyDown={e => {
            if (e.key === 'Enter') {
              if (hours.trim().length > 0) {
                doGoToCommit();
              }
            }
          }}
          ref={hoursInputRef}
        />
      </div>

      <div className="goto-time-or-divider">
        <T>or</T>
      </div>

      <div className="goto-time-datetime-inputs">
        <DatetimePicker
          width="100%"
          value={datetime}
          max={maxDatetime.current}
          onInput={e => handleDatetimeChange((e.target as unknown as {value: string})?.value ?? '')}
          onKeyDown={e => {
            if (e.key === 'Enter' && datetime.trim().length > 0) {
              doGoToCommit();
            } else if (datetime.trim().length === 0) {
              setHours(''); // Clear hours on keyDown, not just onInput, which is only fired for a complete/valid date
            }
          }}
        />
      </div>

      <div className="goto-time-actions">
        <Checkbox checked={shouldRebase} onChange={setShouldRebase}>
          <T>Rebase current stack here</T>
        </Checkbox>
        <Button
          data-testid="goto-time-button"
          primary
          disabled={hours.trim().length === 0 && datetime.trim().length === 0}
          onClick={doGoToCommit}>
          <T>Goto</T>
        </Button>
      </div>
    </div>
  );
}
