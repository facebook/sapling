Test exchange of common information using bundle2


  $ getmainid() {
  >    hg -R main log --template '{node}\n' --rev "$1"
  > }

enable obsolescence

  $ cat > obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > obsolete=$TESTTMP/obs.py
  > [experimental]
  > bundle2-exp=True
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > logtemplate={rev}:{node|short} {phase} {author} {bookmarks} {desc|firstline}
  > [web]
  > push_ssl = false
  > allow_push = *
  > [phases]
  > publish=False
  > [hooks]
  > changegroup = sh -c  "HG_LOCAL= HG_NODE= HG_TAG= python \"$TESTDIR/printenv.py\" changegroup"
  > b2x-transactionclose = sh -c  "HG_LOCAL= HG_NODE= HG_TAG= python \"$TESTDIR/printenv.py\" b2x-transactionclose"
  > EOF

The extension requires a repo (currently unused)

  $ hg init main
  $ cd main
  $ touch a
  $ hg add a
  $ hg commit -m 'a'

  $ hg unbundle $TESTDIR/bundles/rebase.hg
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+3 heads)
  changegroup hook: HG_SOURCE=unbundle HG_URL=bundle:*/tests/bundles/rebase.hg (glob)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ cd ..

Real world exchange
=====================

Add more obsolescence information

  $ hg -R main debugobsolete -d '0 0' 1111111111111111111111111111111111111111 `getmainid 9520eea781bc`
  $ hg -R main debugobsolete -d '0 0' 2222222222222222222222222222222222222222 `getmainid 24b6387c8c8c`

clone --pull

  $ hg -R main phase --public cd010b8cd998
  $ hg clone main other --pull --rev 9520eea781bc
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  1 new obsolescence markers
  changegroup hook: HG_NEW_OBSMARKERS=1 HG_SOURCE=bundle2 HG_URL=bundle2
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R other log -G
  @  1:9520eea781bc draft Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

pull

  $ hg -R main phase --public 9520eea781bc
  $ hg -R other pull -r 24b6387c8c8c
  pulling from $TESTTMP/main (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  1 new obsolescence markers
  changegroup hook: HG_NEW_OBSMARKERS=1 HG_SOURCE=bundle2 HG_URL=bundle2
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R other log -G
  o  2:24b6387c8c8c draft Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  |
  | @  1:9520eea781bc draft Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  2222222222222222222222222222222222222222 24b6387c8c8cae37178880f3fa95ded3cb1cf785 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

pull empty (with phase movement)

  $ hg -R main phase --public 24b6387c8c8c
  $ hg -R other pull -r 24b6387c8c8c
  pulling from $TESTTMP/main (glob)
  no changes found
  $ hg -R other log -G
  o  2:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  |
  | @  1:9520eea781bc draft Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  2222222222222222222222222222222222222222 24b6387c8c8cae37178880f3fa95ded3cb1cf785 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

pull empty

  $ hg -R other pull -r 24b6387c8c8c
  pulling from $TESTTMP/main (glob)
  no changes found
  $ hg -R other log -G
  o  2:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  |
  | @  1:9520eea781bc draft Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  2222222222222222222222222222222222222222 24b6387c8c8cae37178880f3fa95ded3cb1cf785 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

add extra data to test their exchange during push

  $ hg -R main bookmark --rev eea13746799a book_eea1
  $ hg -R main debugobsolete -d '0 0' 3333333333333333333333333333333333333333 `getmainid eea13746799a`
  $ hg -R main bookmark --rev 02de42196ebe book_02de
  $ hg -R main debugobsolete -d '0 0' 4444444444444444444444444444444444444444 `getmainid 02de42196ebe`
  $ hg -R main bookmark --rev 42ccdea3bb16 book_42cc
  $ hg -R main debugobsolete -d '0 0' 5555555555555555555555555555555555555555 `getmainid 42ccdea3bb16`
  $ hg -R main bookmark --rev 5fddd98957c8 book_5fdd
  $ hg -R main debugobsolete -d '0 0' 6666666666666666666666666666666666666666 `getmainid 5fddd98957c8`
  $ hg -R main bookmark --rev 32af7686d403 book_32af
  $ hg -R main debugobsolete -d '0 0' 7777777777777777777777777777777777777777 `getmainid 32af7686d403`

  $ hg -R other bookmark --rev cd010b8cd998 book_eea1
  $ hg -R other bookmark --rev cd010b8cd998 book_02de
  $ hg -R other bookmark --rev cd010b8cd998 book_42cc
  $ hg -R other bookmark --rev cd010b8cd998 book_5fdd
  $ hg -R other bookmark --rev cd010b8cd998 book_32af

  $ hg -R main phase --public eea13746799a

push
  $ hg -R main push other --rev eea13746799a --bookmark book_eea1
  pushing to other
  searching for changes
  b2x-transactionclose hook: HG_BUNDLE2-EXP=1 HG_NEW_OBSMARKERS=1 HG_SOURCE=push HG_URL=push
  changegroup hook: HG_BUNDLE2-EXP=1 HG_NEW_OBSMARKERS=1 HG_SOURCE=bundle2 HG_URL=bundle2
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 0 changes to 0 files (-1 heads)
  remote: 1 new obsolescence markers
  updating bookmark book_eea1
  $ hg -R other log -G
  o    3:eea13746799a public Nicolas Dumazet <nicdumz.commits@gmail.com> book_eea1 G
  |\
  | o  2:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  | |
  @ |  1:9520eea781bc public Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com> book_02de book_32af book_42cc book_5fdd A
  
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  2222222222222222222222222222222222222222 24b6387c8c8cae37178880f3fa95ded3cb1cf785 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  3333333333333333333333333333333333333333 eea13746799a9e0bfd88f29d3c2e9dc9389f524f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

pull over ssh

  $ hg -R other pull ssh://user@dummy/main -r 02de42196ebe --bookmark book_02de
  pulling from ssh://user@dummy/main
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  1 new obsolescence markers
  updating bookmark book_02de
  changegroup hook: HG_NEW_OBSMARKERS=1 HG_SOURCE=bundle2 HG_URL=bundle2
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  2222222222222222222222222222222222222222 24b6387c8c8cae37178880f3fa95ded3cb1cf785 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  3333333333333333333333333333333333333333 eea13746799a9e0bfd88f29d3c2e9dc9389f524f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  4444444444444444444444444444444444444444 02de42196ebee42ef284b6780a87cdc96e8eaab6 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

pull over http

  $ hg -R main serve -p $HGPORT -d --pid-file=main.pid -E main-error.log
  $ cat main.pid >> $DAEMON_PIDS

  $ hg -R other pull http://localhost:$HGPORT/ -r 42ccdea3bb16 --bookmark book_42cc
  pulling from http://localhost:$HGPORT/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  1 new obsolescence markers
  updating bookmark book_42cc
  changegroup hook: HG_NEW_OBSMARKERS=1 HG_SOURCE=bundle2 HG_URL=bundle2
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ cat main-error.log
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  2222222222222222222222222222222222222222 24b6387c8c8cae37178880f3fa95ded3cb1cf785 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  3333333333333333333333333333333333333333 eea13746799a9e0bfd88f29d3c2e9dc9389f524f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  4444444444444444444444444444444444444444 02de42196ebee42ef284b6780a87cdc96e8eaab6 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  5555555555555555555555555555555555555555 42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

push over ssh

  $ hg -R main push ssh://user@dummy/other -r 5fddd98957c8 --bookmark book_5fdd
  pushing to ssh://user@dummy/other
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: 1 new obsolescence markers
  updating bookmark book_5fdd
  remote: b2x-transactionclose hook: HG_BUNDLE2-EXP=1 HG_NEW_OBSMARKERS=1 HG_SOURCE=serve HG_URL=remote:ssh:127.0.0.1
  remote: changegroup hook: HG_BUNDLE2-EXP=1 HG_NEW_OBSMARKERS=1 HG_SOURCE=bundle2 HG_URL=bundle2
  $ hg -R other log -G
  o  6:5fddd98957c8 draft Nicolas Dumazet <nicdumz.commits@gmail.com> book_5fdd C
  |
  o  5:42ccdea3bb16 draft Nicolas Dumazet <nicdumz.commits@gmail.com> book_42cc B
  |
  | o  4:02de42196ebe draft Nicolas Dumazet <nicdumz.commits@gmail.com> book_02de H
  | |
  | | o  3:eea13746799a public Nicolas Dumazet <nicdumz.commits@gmail.com> book_eea1 G
  | |/|
  | o |  2:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  |/ /
  | @  1:9520eea781bc public Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com> book_32af A
  
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  2222222222222222222222222222222222222222 24b6387c8c8cae37178880f3fa95ded3cb1cf785 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  3333333333333333333333333333333333333333 eea13746799a9e0bfd88f29d3c2e9dc9389f524f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  4444444444444444444444444444444444444444 02de42196ebee42ef284b6780a87cdc96e8eaab6 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  5555555555555555555555555555555555555555 42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6666666666666666666666666666666666666666 5fddd98957c8a54a4d436dfe1da9d87f21a1b97b 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

push over http

  $ hg -R other serve -p $HGPORT2 -d --pid-file=other.pid -E other-error.log
  $ cat other.pid >> $DAEMON_PIDS

  $ hg -R main phase --public 32af7686d403
  $ hg -R main push http://localhost:$HGPORT2/ -r 32af7686d403 --bookmark book_32af
  pushing to http://localhost:$HGPORT2/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: 1 new obsolescence markers
  updating bookmark book_32af
  $ cat other-error.log

Check final content.

  $ hg -R other log -G
  o  7:32af7686d403 public Nicolas Dumazet <nicdumz.commits@gmail.com> book_32af D
  |
  o  6:5fddd98957c8 public Nicolas Dumazet <nicdumz.commits@gmail.com> book_5fdd C
  |
  o  5:42ccdea3bb16 public Nicolas Dumazet <nicdumz.commits@gmail.com> book_42cc B
  |
  | o  4:02de42196ebe draft Nicolas Dumazet <nicdumz.commits@gmail.com> book_02de H
  | |
  | | o  3:eea13746799a public Nicolas Dumazet <nicdumz.commits@gmail.com> book_eea1 G
  | |/|
  | o |  2:24b6387c8c8c public Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  |/ /
  | @  1:9520eea781bc public Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  o  0:cd010b8cd998 public Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  $ hg -R other debugobsolete
  1111111111111111111111111111111111111111 9520eea781bcca16c1e15acc0ba14335a0e8e5ba 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  2222222222222222222222222222222222222222 24b6387c8c8cae37178880f3fa95ded3cb1cf785 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  3333333333333333333333333333333333333333 eea13746799a9e0bfd88f29d3c2e9dc9389f524f 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  4444444444444444444444444444444444444444 02de42196ebee42ef284b6780a87cdc96e8eaab6 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  5555555555555555555555555555555555555555 42ccdea3bb16d28e1848c95fe2e44c000f3f21b1 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  6666666666666666666666666666666666666666 5fddd98957c8a54a4d436dfe1da9d87f21a1b97b 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  7777777777777777777777777777777777777777 32af7686d403cf45b5d95f2d70cebea587ac806a 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}

Error Handling
==============

Check that errors are properly returned to the client during push.

Setting up

  $ cat > failpush.py << EOF
  > """A small extension that makes push fails when using bundle2
  > 
  > used to test error handling in bundle2
  > """
  > 
  > from mercurial import util
  > from mercurial import bundle2
  > from mercurial import exchange
  > from mercurial import extensions
  > 
  > def _pushbundle2failpart(pushop, bundler):
  >     reason = pushop.ui.config('failpush', 'reason', None)
  >     part = None
  >     if reason == 'abort':
  >         bundler.newpart('test:abort')
  >     if reason == 'unknown':
  >         bundler.newpart('TEST:UNKNOWN')
  >     if reason == 'race':
  >         # 20 Bytes of crap
  >         bundler.newpart('b2x:check:heads', data='01234567890123456789')
  > 
  > @bundle2.parthandler("test:abort")
  > def handleabort(op, part):
  >     raise util.Abort('Abandon ship!', hint="don't panic")
  > 
  > def uisetup(ui):
  >     exchange.b2partsgenmapping['failpart'] = _pushbundle2failpart
  >     exchange.b2partsgenorder.insert(0, 'failpart')
  > 
  > EOF

  $ cd main
  $ hg up tip
  3 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'I' > I
  $ hg add I
  $ hg ci -m 'I'
  $ hg id
  e7ec4e813ba6 tip
  $ cd ..

  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > failpush=$TESTTMP/failpush.py
  > EOF

  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  $ hg -R other serve -p $HGPORT2 -d --pid-file=other.pid -E other-error.log
  $ cat other.pid >> $DAEMON_PIDS

Doing the actual push: Abort error

  $ cat << EOF >> $HGRCPATH
  > [failpush]
  > reason = abort
  > EOF

  $ hg -R main push other -r e7ec4e813ba6
  pushing to other
  searching for changes
  abort: Abandon ship!
  (don't panic)
  [255]

  $ hg -R main push ssh://user@dummy/other -r e7ec4e813ba6
  pushing to ssh://user@dummy/other
  searching for changes
  abort: Abandon ship!
  (don't panic)
  [255]

  $ hg -R main push http://localhost:$HGPORT2/ -r e7ec4e813ba6
  pushing to http://localhost:$HGPORT2/
  searching for changes
  abort: Abandon ship!
  (don't panic)
  [255]


Doing the actual push: unknown mandatory parts

  $ cat << EOF >> $HGRCPATH
  > [failpush]
  > reason = unknown
  > EOF

  $ hg -R main push other -r e7ec4e813ba6
  pushing to other
  searching for changes
  abort: missing support for test:unknown
  [255]

  $ hg -R main push ssh://user@dummy/other -r e7ec4e813ba6
  pushing to ssh://user@dummy/other
  searching for changes
  abort: missing support for test:unknown
  [255]

  $ hg -R main push http://localhost:$HGPORT2/ -r e7ec4e813ba6
  pushing to http://localhost:$HGPORT2/
  searching for changes
  abort: missing support for test:unknown
  [255]

Doing the actual push: race

  $ cat << EOF >> $HGRCPATH
  > [failpush]
  > reason = race
  > EOF

  $ hg -R main push other -r e7ec4e813ba6
  pushing to other
  searching for changes
  abort: push failed:
  'repository changed while pushing - please try again'
  [255]

  $ hg -R main push ssh://user@dummy/other -r e7ec4e813ba6
  pushing to ssh://user@dummy/other
  searching for changes
  abort: push failed:
  'repository changed while pushing - please try again'
  [255]

  $ hg -R main push http://localhost:$HGPORT2/ -r e7ec4e813ba6
  pushing to http://localhost:$HGPORT2/
  searching for changes
  abort: push failed:
  'repository changed while pushing - please try again'
  [255]

Doing the actual push: hook abort

  $ cat << EOF >> $HGRCPATH
  > [failpush]
  > reason =
  > [hooks]
  > b2x-pretransactionclose.failpush = false
  > EOF

  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  $ hg -R other serve -p $HGPORT2 -d --pid-file=other.pid -E other-error.log
  $ cat other.pid >> $DAEMON_PIDS

  $ hg -R main push other -r e7ec4e813ba6
  pushing to other
  searching for changes
  transaction abort!
  rollback completed
  changegroup hook: HG_BUNDLE2-EXP=1 HG_NEW_OBSMARKERS=0 HG_SOURCE=bundle2 HG_URL=bundle2
  abort: b2x-pretransactionclose.failpush hook exited with status 1
  [255]

  $ hg -R main push ssh://user@dummy/other -r e7ec4e813ba6
  pushing to ssh://user@dummy/other
  searching for changes
  abort: b2x-pretransactionclose.failpush hook exited with status 1
  remote: transaction abort!
  remote: rollback completed
  remote: changegroup hook: HG_BUNDLE2-EXP=1 HG_NEW_OBSMARKERS=0 HG_SOURCE=bundle2 HG_URL=bundle2
  [255]

  $ hg -R main push http://localhost:$HGPORT2/ -r e7ec4e813ba6
  pushing to http://localhost:$HGPORT2/
  searching for changes
  abort: b2x-pretransactionclose.failpush hook exited with status 1
  [255]


