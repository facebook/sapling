#testcases withpytz withoutpytz
#if withpytz
  $ hg debugshell -c "import pytz; pytz.__name__" || exit 80
#endif
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > smartlog=
  > EOF
  $ TZ=UTC
  $ export TZ

Create a repo with some commits at interesting dates.
  $ hg init repo
  $ cd repo
  $ for d in 1970-01-01T00:00 1996-02-29T15:00 1996-03-01T19:20 1996-03-06T08:00 1996-03-07T12:02 1996-03-07T13:56 1996-03-07T14:00 1996-03-07T23:59
  > do
  >   echo "$d" > stamp
  >   hg commit -Aqm "$d" -d "$d"
  > done

Test smartlog with smartdate.
  $ hg smartlog -r 'all()' -T '{rev}: {smartdate(date, 5400, age(date), simpledate(date))} "{desc}"' --config extensions.fakedate=$TESTDIR/fakedate.py
  @  7: 1996-03-07 23:59 "1996-03-07T23:59"
  |
  o  6: 1 second ago "1996-03-07T14:00"
  |
  o  5: 4 minutes ago "1996-03-07T13:56"
  |
  o  4: Today at 12:02 "1996-03-07T12:02"
  |
  o  3: Yesterday at 08:00 "1996-03-06T08:00"
  |
  o  2: Friday at 19:20 "1996-03-01T19:20"
  |
  o  1: Feb 29 at 15:00 "1996-02-29T15:00"
  |
  o  0: 1970-01-01 00:00 "1970-01-01T00:00"
  
  $ hg smartlog -r 'all()' -T '{rev}: {smartdate(date, 18000, age(date), simpledate(date))} "{desc}"' --config extensions.fakedate=$TESTDIR/fakedate.py
  @  7: 1996-03-07 23:59 "1996-03-07T23:59"
  |
  o  6: 1 second ago "1996-03-07T14:00"
  |
  o  5: 4 minutes ago "1996-03-07T13:56"
  |
  o  4: 118 minutes ago "1996-03-07T12:02"
  |
  o  3: Yesterday at 08:00 "1996-03-06T08:00"
  |
  o  2: Friday at 19:20 "1996-03-01T19:20"
  |
  o  1: Feb 29 at 15:00 "1996-02-29T15:00"
  |
  o  0: 1970-01-01 00:00 "1970-01-01T00:00"
  
  $ TZ=America/Los_Angeles hg smartlog -r 'all()' -T '{rev}: {smartdate(date, 5400, age(date), simpledate(date))} "{desc}"' --config extensions.fakedate=$TESTDIR/fakedate.py
  @  7: 1996-03-07 15:59 "1996-03-07T23:59"
  |
  o  6: 1 second ago "1996-03-07T14:00"
  |
  o  5: 4 minutes ago "1996-03-07T13:56"
  |
  o  4: Today at 04:02 "1996-03-07T12:02"
  |
  o  3: Yesterday at 00:00 "1996-03-06T08:00"
  |
  o  2: Friday at 11:20 "1996-03-01T19:20"
  |
  o  1: Feb 29 at 07:00 "1996-02-29T15:00"
  |
  o  0: 1969-12-31 16:00 "1970-01-01T00:00"
  
#if withpytz
  $ hg smartlog -r 'all()' -T '{rev}: {smartdate(date, 5400, age(date), simpledate(date, "Australia/Sydney"))} "{desc}"' --config extensions.fakedate=$TESTDIR/fakedate.py
  @  7: 1996-03-08 10:59 "1996-03-07T23:59"
  |
  o  6: 1 second ago "1996-03-07T14:00"
  |
  o  5: 4 minutes ago "1996-03-07T13:56"
  |
  o  4: Yesterday at 23:02 "1996-03-07T12:02"
  |
  o  3: Wednesday at 19:00 "1996-03-06T08:00"
  |
  o  2: Saturday at 06:20 "1996-03-01T19:20"
  |
  o  1: Mar 01 at 02:00 "1996-02-29T15:00"
  |
  o  0: 1970-01-01 10:00 "1970-01-01T00:00"
  
#endif
