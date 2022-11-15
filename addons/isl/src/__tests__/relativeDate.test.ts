/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import {setCurrentLanguage} from '../i18n';
import {relativeDate} from '../relativeDate';

describe('relativeDate', () => {
  const SECOND = 1000;
  const MINUTE = 60 * SECOND;
  const HOUR = 60 * MINUTE;
  const DAY = 24 * HOUR;
  const WEEK = 7 * DAY;
  const YEAR = DAY * 365;
  const MONTH = YEAR / 12;

  const reference = 157765000000; // 01.01.1975 00:00
  const now = new Date().getTime();

  const check = (time: number) => expect(relativeDate(time, {reference}));
  const checkShort = (time: number) =>
    expect(relativeDate(time, {reference, useShortVariant: true}));

  describe('en', () => {
    beforeAll(() => {
      setCurrentLanguage('en');
    });

    it('renders relative dates', () => {
      expect(relativeDate(new Date(), {})).toEqual('just now');

      // test long format
      expect(relativeDate(0, {})).toEqual(Math.round(now / YEAR) + ' years ago');
      check(reference - 41 * SECOND).toEqual('just now');
      check(reference - 42 * SECOND).toEqual('a minute ago');
      check(reference - MINUTE).toEqual('a minute ago');
      check(reference - MINUTE * 1.5).toEqual('2 minutes ago');
      check(reference - MINUTE * 59).toEqual('59 minutes ago');
      check(reference - HOUR).toEqual('an hour ago');
      check(reference - HOUR * 1.5).toEqual('2 hours ago');
      check(reference - HOUR * 16).toEqual('16 hours ago');
      check(reference - HOUR * 23).toEqual('23 hours ago');
      check(reference - DAY * 1.8).toEqual('yesterday');
      check(reference - DAY * 3).toEqual('3 days ago');
      check(reference - DAY * 6).toEqual('6 days ago');
      check(reference - WEEK).toEqual('a week ago');
      check(reference - WEEK * 2).toEqual('2 weeks ago');
      check(reference - WEEK * 4).toEqual('4 weeks ago');
      check(reference - MONTH * 1.2).toEqual('a month ago');
      check(reference - YEAR + HOUR).toEqual('12 months ago');
      check(reference - YEAR).toEqual('a year ago');
      check(reference - YEAR * 2).toEqual('2 years ago');
    });

    it('renders short relative dates', () => {
      // test short format
      checkShort(reference - 41 * SECOND).toEqual('now');
      checkShort(reference - 42 * SECOND).toEqual('1m');
      checkShort(reference - MINUTE).toEqual('1m');
      checkShort(reference - MINUTE * 1.5).toEqual('2m');
      checkShort(reference - MINUTE * 59).toEqual('59m');
      checkShort(reference - HOUR).toEqual('1h');
      checkShort(reference - HOUR * 1.5).toEqual('2h');
      checkShort(reference - HOUR * 16).toEqual('16h');
      checkShort(reference - HOUR * 23).toEqual('23h');
      checkShort(reference - DAY * 1.8).toEqual('1d');
      checkShort(reference - DAY * 3).toEqual('3d');
      checkShort(reference - DAY * 6).toEqual('6d');
      checkShort(reference - WEEK).toEqual('1w');
      checkShort(reference - WEEK * 2).toEqual('2w');
      checkShort(reference - WEEK * 4).toEqual('4w');
      checkShort(reference - MONTH * 1.2).toEqual('1mo');
      checkShort(reference - YEAR + HOUR).toEqual('12mo');
      checkShort(reference - YEAR).toEqual('1y');
      checkShort(reference - YEAR * 2).toEqual('2y');
    });
  });

  describe('de', () => {
    beforeAll(() => {
      setCurrentLanguage('de');
    });
    it('renders relative dates', () => {
      check(reference - 41 * SECOND).toEqual('vor 1 Min.');
      check(reference - 42 * SECOND).toEqual('vor 1 Min.');
      check(reference - MINUTE).toEqual('vor 1 Min.');
      check(reference - MINUTE * 1.5).toEqual('vor 2 Min.');
      check(reference - MINUTE * 59).toEqual('vor 59 Min.');
      check(reference - HOUR).toEqual('vor 60 Min.');
      check(reference - HOUR * 1.5).toEqual('vor 2 Std.');
      check(reference - HOUR * 16).toEqual('vor 16 Std.');
      check(reference - HOUR * 23).toEqual('vor 23 Std.');
      check(reference - DAY * 1.8).toEqual('vorgestern');
      check(reference - DAY * 3).toEqual('vor 3 Tagen');
      check(reference - DAY * 6).toEqual('vor 6 Tagen');
      check(reference - WEEK).toEqual('vor 7 Tagen');
      check(reference - WEEK * 2).toEqual('vor 14 Tagen');
      check(reference - WEEK * 4).toEqual('vor 28 Tagen');
      check(reference - MONTH * 1.2).toEqual('letzten Monat');
      //   check(reference - YEAR + HOUR).toEqual('vor 12 Monaten'); // some kind of whitespace issue
      //   check(reference - YEAR).toEqual('vor 12 Monaten'); // some kind of whitespace issue
      check(reference - YEAR * 2).toEqual('vor 2 Jahren');

      check(reference + 2 * HOUR).toEqual('in 2 Std.');
      check(reference + MINUTE).toEqual('in 1 Min.');
    });
  });
});
