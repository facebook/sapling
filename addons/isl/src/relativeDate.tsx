/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {getCurrentLanguage, useCurrentLang} from './i18n';

/**
 * Originally adapted from https://github.com/azer/relative-date.
 */
const SECOND = 1000;
const MINUTE = SECOND * 60;
const HOUR = MINUTE * 60;
const DAY = HOUR * 24;
const WEEK = DAY * 7;
const YEAR = DAY * 365;
const MONTH = YEAR / 12;

type NumberFormat = [ms: number, name: string] | [ms: number, name: string, relative: number];

const shortFormats: Array<NumberFormat> = [
  [MINUTE * 0.7, 'now'],
  [MINUTE * 1.5, '1m'],
  [MINUTE * 60, 'm', MINUTE],
  [HOUR * 1.5, '1h'],
  [DAY, 'h', HOUR],
  [DAY * 2, '1d'],
  [DAY * 7, 'd', DAY],
  [WEEK * 1.5, '1w'],
  [MONTH, 'w', WEEK],
  [MONTH * 1.5, '1mo'],
  [YEAR, 'mo', MONTH],
  [YEAR * 1.5, '1y'],
  [Number.MAX_VALUE, 'y', YEAR],
];

const longFormatsRelative: Array<NumberFormat> = [
  [MINUTE * 0.7, 'less than a minute'],
  [MINUTE * 1.5, 'one minute'],
  [MINUTE * 60, 'minutes', MINUTE],
  [HOUR * 1.5, 'one hour'],
  [DAY, 'hours', HOUR],
  [DAY * 2, 'one day'],
  [DAY * 7, 'days', DAY],
  [WEEK * 1.5, 'one week'],
  [MONTH, 'weeks', WEEK],
  [MONTH * 1.5, 'one month'],
  [YEAR, 'months', MONTH],
  [YEAR * 1.5, 'one year'],
  [Number.MAX_VALUE, 'years', YEAR],
];

const longFormats: Array<NumberFormat> = [
  [MINUTE * 0.7, 'just now'],
  [MINUTE * 1.5, 'a minute ago'],
  [MINUTE * 60, 'minutes ago', MINUTE],
  [HOUR * 1.5, 'an hour ago'],
  [DAY, 'hours ago', HOUR],
  [DAY * 2, 'yesterday'],
  [DAY * 7, 'days ago', DAY],
  [WEEK * 1.5, 'a week ago'],
  [MONTH, 'weeks ago', WEEK],
  [MONTH * 1.5, 'a month ago'],
  [YEAR, 'months ago', MONTH],
  [YEAR * 1.5, 'a year ago'],
  [Number.MAX_VALUE, 'years ago', YEAR],
];

const longFormatsNumbers: Array<NumberFormat> = [
  [MINUTE * 0.7, 'just now'],
  [MINUTE * 1.5, '1 minute ago'],
  [MINUTE * 60, 'minutes ago', MINUTE],
  [HOUR * 1.5, '1 hour ago'],
  [DAY, 'hours ago', HOUR],
  [DAY * 2, 'yesterday'],
  [DAY * 7, 'days ago', DAY],
  [WEEK * 1.5, '1 week ago'],
  [MONTH, 'weeks ago', WEEK],
  [MONTH * 1.5, '1 month ago'],
  [YEAR, 'months ago', MONTH],
  [YEAR * 1.5, '1 year ago'],
  [Number.MAX_VALUE, 'years ago', YEAR],
];

const units = {
  year: 24 * 60 * 60 * 1000 * 365,
  month: (24 * 60 * 60 * 1000 * 365) / 12,
  day: 24 * 60 * 60 * 1000,
  hour: 60 * 60 * 1000,
  minute: 60 * 1000,
};

/**
 * Format date into relative string format.
 * If currentLanguage is 'en', uses hard-coded time abbrviations for maximum shortness.
 * Other langauges use currently Intl.RelativeTimeFormat if available.
 * if currentLanguage is 'en':
 * ```
 * relativeDate(new Date()) -> 'just now'
 * relativeDate(new Date() - 120000) -> '2m ago'
 * ```
 * if currentLanguage is 'de':
 * ```
 * relativeDate(new Date()) -> 'just now'
 * relativeDate(new Date() - 60000) -> 'now'
 * ```
 */
export function relativeDate(
  input_: number | Date,
  options: {
    reference?: number | Date;
    useShortVariant?: boolean;
    useNumbersOnly?: boolean;
    useRelativeForm?: boolean;
  },
): string {
  let input = input_;
  let reference = options.reference;
  if (input instanceof Date) {
    input = input.getTime();
  }
  if (!reference) {
    reference = new Date().getTime();
  }
  if (reference instanceof Date) {
    reference = reference.getTime();
  }

  const delta = reference - input;

  // Use Intl.RelativeTimeFormat for non-en locales, if available.
  if (getCurrentLanguage() != 'en' && typeof Intl !== undefined) {
    for (const unit of Object.keys(units) as Array<keyof typeof units>) {
      if (Math.abs(delta) > units[unit] || unit == 'minute') {
        return new Intl.RelativeTimeFormat(getCurrentLanguage(), {
          style: options.useShortVariant ? 'narrow' : 'short',
          numeric: 'auto',
        }).format(-Math.round(delta / units[unit]), unit);
      }
    }
  }

  const formats = options.useRelativeForm
    ? longFormatsRelative
    : options.useShortVariant
    ? shortFormats
    : options.useNumbersOnly
    ? longFormatsNumbers
    : longFormats;
  for (const [limit, relativeFormat, remainder] of formats) {
    if (delta < limit) {
      if (typeof remainder === 'number') {
        return (
          Math.round(delta / remainder) + (options.useShortVariant ? '' : ' ') + relativeFormat
        );
      } else {
        return relativeFormat;
      }
    }
  }

  throw new Error('This should never be reached.');
}

/**
 * React component version of {@link relativeDate}.
 * Re-renders if the current language changes.
 */
export function RelativeDate({
  date,
  reference,
  useShortVariant,
  useNumbersOnly,
  useRelativeForm,
}: {
  date: number | Date;
  reference?: number | Date;
  useShortVariant?: boolean;
  useNumbersOnly?: boolean;
  useRelativeForm?: boolean;
}) {
  useCurrentLang();
  return <>{relativeDate(date, {reference, useShortVariant, useNumbersOnly, useRelativeForm})}</>;
}
