This runs with TZ="GMT"

  $ hg init
  $ echo "test-parse-date" > a
  $ hg add a
  $ hg ci -d "2006-02-01 13:00:30" -m "rev 0"
  $ echo "hi!" >> a
  $ hg ci -d "2006-02-01 13:00:30 -0500" -m "rev 1"
  $ hg tag -d "2006-04-15 13:30" "Hi"
  $ hg backout --merge -d "2006-04-15 13:30 +0200" -m "rev 3" 1
  reverting a
  created new head
  changeset 3:107ce1ee2b43 backs out changeset 1:25a1420a55f8
  merging with changeset 3:107ce1ee2b43
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -d "1150000000 14400" -m "rev 4 (merge)"
  $ echo "fail" >> a
  $ hg ci -d "should fail" -m "fail"
  abort: invalid date: 'should fail'
  [255]
  $ hg ci -d "100000000000000000 1400" -m "fail"
  abort: date exceeds 32 bits: 100000000000000000
  [255]
  $ hg ci -d "100000 1400000" -m "fail"
  abort: impossible time zone offset: 1400000
  [255]

Check with local timezone other than GMT and with DST

  $ TZ="PST+8PDT"
  $ export TZ

PST=UTC-8 / PDT=UTC-7

  $ hg debugrebuildstate
  $ echo "a" > a
  $ hg ci -d "2006-07-15 13:30" -m "summer@UTC-7"
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

Test date formats with '>' or '<' accompanied by space characters

  $ hg log -d '>' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000
  $ hg log -d '<' hg log -d '>' --template '{date|date}\n'

  $ hg log -d ' >' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000
  $ hg log -d ' <' --template '{date|date}\n'

  $ hg log -d '> ' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000
  $ hg log -d '< ' --template '{date|date}\n'

  $ hg log -d ' > ' --template '{date|date}\n'
  Sun Jan 15 13:30:00 2006 +0500
  Sun Jan 15 13:30:00 2006 -0800
  Sat Jul 15 13:30:00 2006 +0500
  Sat Jul 15 13:30:00 2006 -0700
  Sun Jun 11 00:26:40 2006 -0400
  Sat Apr 15 13:30:00 2006 +0200
  Sat Apr 15 13:30:00 2006 +0000
  Wed Feb 01 13:00:30 2006 -0500
  Wed Feb 01 13:00:30 2006 +0000
  $ hg log -d ' < ' --template '{date|date}\n'

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
