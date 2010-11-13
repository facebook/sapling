
  $ hg init t
  $ cd t
  $ cat > unix2dos.py <<EOF
  > import sys
  > 
  > for path in sys.argv[1:]:
  >     data = file(path, 'rb').read()
  >     data = data.replace('\n', '\r\n')
  >     file(path, 'wb').write(data)
  > EOF
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'pretxncommit.crlf = python:hgext.win32text.forbidcrlf' >> .hg/hgrc
  $ echo 'pretxnchangegroup.crlf = python:hgext.win32text.forbidcrlf' >> .hg/hgrc
  $ cat .hg/hgrc
  [hooks]
  pretxncommit.crlf = python:hgext.win32text.forbidcrlf
  pretxnchangegroup.crlf = python:hgext.win32text.forbidcrlf
  $ echo
  
  $ echo hello > f
  $ hg add f

commit should succeed

  $ hg ci -m 1
  $ echo
  
  $ hg clone . ../zoz
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cp .hg/hgrc ../zoz/.hg
  $ python unix2dos.py f

commit should fail

  $ hg ci -m 2.1
  Attempt to commit or push text file(s) using CRLF line endings
  in f583ea08d42a: f
  transaction abort!
  rollback completed
  abort: pretxncommit.crlf hook failed
  [255]
  $ echo
  
  $ mv .hg/hgrc .hg/hgrc.bak

commits should succeed

  $ hg ci -m 2
  $ hg cp f g
  $ hg ci -m 2.2
  $ echo
  

push should fail

  $ hg push ../zoz
  pushing to ../zoz
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  Attempt to commit or push text file(s) using CRLF line endings
  in bc2d09796734: g
  in b1aa5cde7ff4: f
  
  To prevent this mistake in your local repository,
  add to Mercurial.ini or .hg/hgrc:
  
  [hooks]
  pretxncommit.crlf = python:hgext.win32text.forbidcrlf
  
  and also consider adding:
  
  [extensions]
  win32text =
  [encode]
  ** = cleverencode:
  [decode]
  ** = cleverdecode:
  transaction abort!
  rollback completed
  abort: pretxnchangegroup.crlf hook failed
  [255]
  $ echo
  
  $ mv .hg/hgrc.bak .hg/hgrc
  $ echo hello > f
  $ hg rm g

commit should succeed

  $ hg ci -m 2.3
  $ echo
  

push should succeed

  $ hg push ../zoz
  pushing to ../zoz
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files
  $ echo
  

and now for something completely different

  $ mkdir d
  $ echo hello > d/f2
  $ python unix2dos.py d/f2
  $ hg add d/f2
  $ hg ci -m 3
  Attempt to commit or push text file(s) using CRLF line endings
  in 053ba1a3035a: d/f2
  transaction abort!
  rollback completed
  abort: pretxncommit.crlf hook failed
  [255]
  $ hg revert -a
  forgetting d/f2
  $ rm d/f2
  $ echo
  
  $ hg rem f
  $ hg ci -m 4
  $ echo
  
  $ python -c 'file("bin", "wb").write("hello\x00\x0D\x0A")'
  $ hg add bin
  $ hg ci -m 5
  $ hg log -v
  changeset:   5:f0b1c8d75fce
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bin
  description:
  5
  
  
  changeset:   4:77796dbcd4ad
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  4
  
  
  changeset:   3:7c1b5430b350
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f g
  description:
  2.3
  
  
  changeset:   2:bc2d09796734
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       g
  description:
  2.2
  
  
  changeset:   1:b1aa5cde7ff4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  2
  
  
  changeset:   0:fcf06d5c4e1d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  1
  
  
  $ echo
  
  $ hg clone . dupe
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo
  
  $ for x in a b c d; do echo content > dupe/$x; done
  $ hg -R dupe add
  adding dupe/a
  adding dupe/b
  adding dupe/c
  adding dupe/d
  $ python unix2dos.py dupe/b dupe/c dupe/d
  $ hg -R dupe ci -m a dupe/a
  $ hg -R dupe ci -m b/c dupe/[bc]
  $ hg -R dupe ci -m d dupe/d
  $ hg -R dupe log -v
  changeset:   8:67ac5962ab43
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       d
  description:
  d
  
  
  changeset:   7:68c127d1834e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       b c
  description:
  b/c
  
  
  changeset:   6:adbf8bf7f31d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  a
  
  
  changeset:   5:f0b1c8d75fce
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bin
  description:
  5
  
  
  changeset:   4:77796dbcd4ad
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  4
  
  
  changeset:   3:7c1b5430b350
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f g
  description:
  2.3
  
  
  changeset:   2:bc2d09796734
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       g
  description:
  2.2
  
  
  changeset:   1:b1aa5cde7ff4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  2
  
  
  changeset:   0:fcf06d5c4e1d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  1
  
  
  $ echo
  
  $ hg pull dupe
  pulling from dupe
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 4 changes to 4 files
  Attempt to commit or push text file(s) using CRLF line endings
  in 67ac5962ab43: d
  in 68c127d1834e: b
  in 68c127d1834e: c
  
  To prevent this mistake in your local repository,
  add to Mercurial.ini or .hg/hgrc:
  
  [hooks]
  pretxncommit.crlf = python:hgext.win32text.forbidcrlf
  
  and also consider adding:
  
  [extensions]
  win32text =
  [encode]
  ** = cleverencode:
  [decode]
  ** = cleverdecode:
  transaction abort!
  rollback completed
  abort: pretxnchangegroup.crlf hook failed
  [255]
  $ echo
  
  $ hg log -v
  changeset:   5:f0b1c8d75fce
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bin
  description:
  5
  
  
  changeset:   4:77796dbcd4ad
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  4
  
  
  changeset:   3:7c1b5430b350
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f g
  description:
  2.3
  
  
  changeset:   2:bc2d09796734
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       g
  description:
  2.2
  
  
  changeset:   1:b1aa5cde7ff4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  2
  
  
  changeset:   0:fcf06d5c4e1d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       f
  description:
  1
  
  
  $ echo
  
  $ rm .hg/hgrc
  $ (echo some; echo text) > f3
  $ python -c 'file("f4.bat", "wb").write("rem empty\x0D\x0A")'
  $ hg add f3 f4.bat
  $ hg ci -m 6
  $ cat bin
  hello\x00\r (esc)
  $ cat f3
  some
  text
  $ cat f4.bat
  rem empty\r (esc)
  $ echo
  
  $ echo '[extensions]' >> .hg/hgrc
  $ echo 'win32text = ' >> .hg/hgrc
  $ echo '[decode]' >> .hg/hgrc
  $ echo '** = cleverdecode:' >> .hg/hgrc
  $ echo '[encode]' >> .hg/hgrc
  $ echo '** = cleverencode:' >> .hg/hgrc
  $ cat .hg/hgrc
  [extensions]
  win32text = 
  [decode]
  ** = cleverdecode:
  [encode]
  ** = cleverencode:

Trigger deprecation warning:

  $ hg id -t
  win32text is deprecated: http://mercurial.selenic.com/wiki/Win32TextExtension
  tip

Disable warning:

  $ echo '[win32text]' >> .hg/hgrc
  $ echo 'warn = no' >> .hg/hgrc
  $ hg id -t
  tip

  $ rm f3 f4.bat bin
  $ hg co -C
  WARNING: f4.bat already has CRLF line endings
  and does not need EOL conversion by the win32text plugin.
  Before your next commit, please reconsider your encode/decode settings in 
  Mercurial.ini or $TESTTMP/t/.hg/hgrc.
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat bin
  hello\x00\r (esc)
  $ cat f3
  some\r (esc)
  text\r (esc)
  $ cat f4.bat
  rem empty\r (esc)
  $ echo
  
  $ python -c 'file("f5.sh", "wb").write("# empty\x0D\x0A")'
  $ hg add f5.sh
  $ hg ci -m 7
  $ cat f5.sh
  # empty\r (esc)
  $ hg cat f5.sh
  # empty
  $ echo '% just linefeed' > linefeed
  $ hg ci -qAm 8 linefeed
  $ cat linefeed
  % just linefeed
  $ hg cat linefeed
  % just linefeed
  $ hg st -q
  $ hg revert -a linefeed
  no changes needed to linefeed
  $ cat linefeed
  % just linefeed
  $ hg st -q
  $ echo modified >> linefeed
  $ hg st -q
  M linefeed
  $ hg revert -a
  reverting linefeed
  $ hg st -q
  $ cat linefeed
  % just linefeed\r (esc)
