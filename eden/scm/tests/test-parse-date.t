#chg-compatible

This runs with TZ="GMT"

  $ hg init
  $ echo "test-parse-date" > a
  $ hg add a
  $ hg ci -d "2006-02-01 13:00:30" -m "rev 0"
  $ echo "hi!" >> a
  $ hg ci -d "2006-02-01 13:00:30 -0500" -m "rev 1"
  $ echo >> .hgtags
  $ hg ci -Aq -d "2006-04-15 13:30" -m "Hi"
  $ hg backout --merge -d "2006-04-15 13:30 +0200" -m "rev 3" 1
  reverting a
  changeset 3:cac74e007661 backs out changeset 1:25a1420a55f8
  merging with changeset 3:cac74e007661
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -d "1150000000 14400" -m "rev 4 (merge)"
  $ echo "fail" >> a
  $ hg ci -d "should fail" -m "fail"
  hg: parse error: invalid date: 'should fail'
  [255]
  $ hg ci -d "100000000000000000 1400" -m "fail"
  hg: parse error: invalid date: '100000000000000000 1400'
  [255]
  $ hg ci -d "100000 1400000" -m "fail"
  hg: parse error: invalid date: '100000 1400000'
  [255]

Check with local timezone other than GMT and with DST

  $ TZ="PST+8PDT+7,M4.1.0/02:00:00,M10.5.0/02:00:00"
  $ export TZ

PST=UTC-8 / PDT=UTC-7
Summer time begins on April's first Sunday at 2:00am,
and ends on October's last Sunday at 2:00am.

  $ hg debugrebuildstate
  $ echo "a" > a
#if windows
Windows $TZ handling is a workaround to the time-0.1.42 crate.
It does not support DST. See comments in pyhgtime/src/lib.rs:tzset.
Workaround it in this test by providing timezone explicitly.
  $ hg ci -d "2006-07-15 13:30 -0700" -m "summer@UTC-7"
#else
  $ hg ci -d "2006-07-15 13:30" -m "summer@UTC-7"
#endif
  $ hg debugrebuildstate
  $ echo "b" > a
  $ hg ci -d "2006-07-15 13:30 +0500" -m "summer@UTC+5"
  $ hg debugrebuildstate
  $ echo "c" > a
  $ hg ci -d "2006-01-15 13:30" -m "winter@UTC-8"
  $ hg debugrebuildstate
  $ echo "d" > a
  $ hg ci -d "2006-01-15 13:30 +0500" -m "winter@UTC+5"
  $ hg log --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

Test issue1014 (fractional timezones)

  $ hg debugdate "1000000000 -16200" # 0430
  internal: 1000000000 -16200
  standard: Sun Sep 09 06:16:40 2001 +0430
  $ hg debugdate "1000000000 -15300" # 0415
  internal: 1000000000 -15300
  standard: Sun Sep 09 06:01:40 2001 +0415
  $ hg debugdate "1000000000 -14400" # 0400
  internal: 1000000000 -14400
  standard: Sun Sep 09 05:46:40 2001 +0400
  $ hg debugdate "1000000000 0"      # GMT
  internal: 1000000000 0
  standard: Sun Sep 09 01:46:40 2001 +0000
  $ hg debugdate "1000000000 14400"  # -0400
  internal: 1000000000 14400
  standard: Sat Sep 08 21:46:40 2001 -0400
  $ hg debugdate "1000000000 15300"  # -0415
  internal: 1000000000 15300
  standard: Sat Sep 08 21:31:40 2001 -0415
  $ hg debugdate "1000000000 16200"  # -0430
  internal: 1000000000 16200
  standard: Sat Sep 08 21:16:40 2001 -0430
  $ hg debugdate "Sat Sep 08 21:16:40 2001 +0430"
  internal: 999967600 -16200
  standard: Sat Sep 08 21:16:40 2001 +0430
  $ hg debugdate "Sat Sep 08 21:16:40 2001 -0430"
  internal: 1000000000 16200
  standard: Sat Sep 08 21:16:40 2001 -0430

Test 12-hours times

  $ hg debugdate "2006-02-01 1:00:30PM +0000"
  internal: 1138798830 0
  standard: Wed Feb 01 13:00:30 2006 +0000
  $ hg debugdate "1:00:30PM" > /dev/null

Normal range

  $ hg log -d -1

Negative range

  $ hg log -d "--2"
  hg: parse error: invalid date: '--2'
  [255]

Whitespace only

  $ hg log -d " "
  hg: parse error: invalid date: ' '
  [255]

Test date formats with '>' or '<' accompanied by space characters

  $ hg log -d '>' --template '{date|date}\n'
  hg: parse error: invalid date: '>'
  [255]
  $ hg log -d '<' --template '{date|date}\n'
  hg: parse error: invalid date: '<'
  [255]

  $ hg log -d ' >' --template '{date|date}\n'
  hg: parse error: invalid date: ' >'
  [255]
  $ hg log -d ' <' --template '{date|date}\n'
  hg: parse error: invalid date: ' <'
  [255]

  $ hg log -d '> ' --template '{date|date}\n'
  hg: parse error: invalid date: '> '
  [255]
  $ hg log -d '< ' --template '{date|date}\n'
  hg: parse error: invalid date: '< '
  [255]

  $ hg log -d ' > ' --template '{date|date}\n'
  hg: parse error: invalid date: ' > '
  [255]
  $ hg log -d ' < ' --template '{date|date}\n'
  hg: parse error: invalid date: ' < '
  [255]

  $ hg log -d '>02/01' --template '{date|date}\n'
  $ hg log -d '<02/01' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

  $ hg log -d ' >02/01' --template '{date|date}\n'
  $ hg log -d ' <02/01' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

  $ hg log -d '> 02/01' --template '{date|date}\n'
  $ hg log -d '< 02/01' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

  $ hg log -d ' > 02/01' --template '{date|date}\n'
  $ hg log -d ' < 02/01' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

  $ hg log -d '>02/01 ' --template '{date|date}\n'
  $ hg log -d '<02/01 ' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

  $ hg log -d ' >02/01 ' --template '{date|date}\n'
  $ hg log -d ' <02/01 ' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

  $ hg log -d '> 02/01 ' --template '{date|date}\n'
  $ hg log -d '< 02/01 ' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

  $ hg log -d ' > 02/01 ' --template '{date|date}\n'
  $ hg log -d ' < 02/01 ' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000

Test issue 3764 (interpreting 'today' and 'yesterday')
  $ echo "hello" >> a
  >>> import datetime
  >>> today = datetime.date.today().strftime("%b %d")
  >>> yesterday = (datetime.date.today() - datetime.timedelta(days=1)).strftime("%b %d")
  >>> dates = open('dates', 'w')
  >>> dates.write(today + '\n')
  >>> dates.write(yesterday + '\n')
  >>> dates.close()
  $ hg ci -d "`sed -n '1p' dates`" -m "today is a good day to code"
  $ hg log -d today --template '{desc}\n'
  today is a good day to code
  $ echo "goodbye" >> a
  $ hg ci -d "`sed -n '2p' dates`" -m "the time traveler's code"
  $ hg log -d yesterday --template '{desc}\n'
  the time traveler's code
  $ echo "foo" >> a
  $ hg commit -d now -m 'Explicitly committed now.'
  $ hg log -d today --template '{desc}\n'
  Explicitly committed now.
  today is a good day to code

Test parsing various ISO8601 forms

#if no-windows
(TZ with DST does not work on Windows)
  $ hg debugdate "2016-07-27T12:10:21"
  internal: 1469646621 * (glob)
  standard: Wed Jul 27 12:10:21 2016 -0700
#endif
  $ hg debugdate "2016-07-27T12:10:21Z"
  internal: 1469621421 0
  standard: Wed Jul 27 12:10:21 2016 +0000
  $ hg debugdate "2016-07-27T12:10:21+00:00"
  internal: 1469621421 0
  standard: Wed Jul 27 12:10:21 2016 +0000
  $ hg debugdate "2016-07-27T121021Z"
  internal: 1469621421 0
  standard: Wed Jul 27 12:10:21 2016 +0000

#if no-windows
(TZ with DST does not work on Windows)
  $ hg debugdate "2016-07-27 12:10:21"
  internal: 1469646621 * (glob)
  standard: Wed Jul 27 12:10:21 2016 -0700
#endif
  $ hg debugdate "2016-07-27 12:10:21Z"
  internal: 1469621421 0
  standard: Wed Jul 27 12:10:21 2016 +0000
  $ hg debugdate "2016-07-27 12:10:21+00:00"
  internal: 1469621421 0
  standard: Wed Jul 27 12:10:21 2016 +0000
  $ hg debugdate "2016-07-27 121021Z"
  internal: 1469621421 0
  standard: Wed Jul 27 12:10:21 2016 +0000

Test parsing extra forms

  $ setconfig extensions.fakedate="$TESTDIR/fakedate.py"
  $ hg debugdate 'now'
  internal: 826207201 0
  standard: Thu Mar 07 14:00:01 1996 +0000
  $ hg debugdate '6h ago'
  internal: 826185601 0
  standard: Thu Mar 07 08:00:01 1996 +0000
  $ hg debugdate --range 'today'
  start: 826185600 (Thu Mar 07 00:00:00 1996 -0800)
    end: 826272000 (Fri Mar 08 00:00:00 1996 -0800)
  $ hg debugdate --range 'yesterday to today'
  start: 826099200 (Wed Mar 06 00:00:00 1996 -0800)
    end: 826272000 (Fri Mar 08 00:00:00 1996 -0800)
  $ hg debugdate --range 'since 5 months ago'
  start: 813057121 (Sat Oct 07 09:12:01 1995 +0000)
    end: 253402300799 (Fri Dec 31 23:59:59 9999 +0000)
  $ hg debugdate --range '2000 to 2001'
  start: 946713600 (Sat Jan 01 00:00:00 2000 -0800)
    end: 1009872000 (Tue Jan 01 00:00:00 2002 -0800)
  $ hg debugdate --range 'before 2001'
  start: -2208988800 (Mon Jan 01 00:00:00 1900 +0000)
    end: 978336000 (Mon Jan 01 00:00:00 2001 -0800)

#if no-windows
(Windows parsing are affected by System Local settings that are unclear how to
override in tests. Cannot assume English here.)

Test parsing months

  $ for i in Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec; do
  >   hg log -d "$i 2018" -r null
  > done
#endif
