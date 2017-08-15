setup repo
  $ hg init t
  $ cd t
  $ echo a > a
  $ hg commit -Am'add a'
  adding a
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  $ hg parents
  changeset:   0:1f0dee641bb7
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  

rollback to null revision
  $ hg status
  $ hg rollback
  repository tip rolled back to revision -1 (undo commit)
  working directory now based on revision -1
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  0 files, 0 changesets, 0 total revisions
  $ hg parents
  $ hg status
  A a

Two changesets this time so we rollback to a real changeset
  $ hg commit -m'add a again'
  $ echo a >> a
  $ hg commit -m'modify a'

Test issue 902 (current branch is preserved)
  $ hg branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ hg rollback
  repository tip rolled back to revision 0 (undo commit)
  working directory now based on revision 0
  $ hg branch
  default

Test issue 1635 (commit message saved)
  $ cat .hg/last-message.txt ; echo
  modify a

Test rollback of hg before issue 902 was fixed

  $ hg commit -m "test3"
  $ hg branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ rm .hg/undo.branch
  $ hg rollback
  repository tip rolled back to revision 0 (undo commit)
  named branch could not be reset: current branch is still 'test'
  working directory now based on revision 0
  $ hg branch
  test

working dir unaffected by rollback: do not restore dirstate et. al.
  $ hg log --template '{rev}  {branch}  {desc|firstline}\n'
  0  default  add a again
  $ hg status
  M a
  $ hg bookmark foo
  $ hg commit -m'modify a again'
  $ echo b > b
  $ hg bookmark bar -r default #making bar active, before the transaction
  $ hg commit -Am'add b'
  adding b
  $ hg log --template '{rev}  {branch}  {desc|firstline}\n'
  2  test  add b
  1  test  modify a again
  0  default  add a again
  $ hg update bar
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark bar)
  $ cat .hg/undo.branch ; echo
  test
  $ hg rollback -f
  repository tip rolled back to revision 1 (undo commit)
  $ hg id -n
  0
  $ hg branch
  default
  $ cat .hg/bookmarks.current ; echo
  bar
  $ hg bookmark --delete foo bar

rollback by pretxncommit saves commit message (issue1635)

  $ echo a >> a
  $ hg --config hooks.pretxncommit=false commit -m"precious commit message"
  transaction abort!
  rollback completed
  abort: pretxncommit hook exited with status * (glob)
  [255]
  $ cat .hg/last-message.txt ; echo
  precious commit message

same thing, but run $EDITOR

  $ cat > editor.sh << '__EOF__'
  > echo "another precious commit message" > "$1"
  > __EOF__
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg --config hooks.pretxncommit=false commit 2>&1
  note: commit message saved in .hg/last-message.txt
  transaction abort!
  rollback completed
  abort: pretxncommit hook exited with status * (glob)
  [255]
  $ cat .hg/last-message.txt
  another precious commit message

test rollback on served repository

#if serve
  $ hg commit -m "precious commit message"
  $ hg serve -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..
  $ hg clone http://localhost:$HGPORT u
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 1 files (+1 heads)
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd u
  $ hg id default
  068774709090

now rollback and observe that 'hg serve' reloads the repository and
presents the correct tip changeset:

  $ hg -R ../t rollback
  repository tip rolled back to revision 1 (undo commit)
  working directory now based on revision 0
  $ hg id default
  791dd2169706

  $ killdaemons.py
#endif

update to older changeset and then refuse rollback, because
that would lose data (issue2998)
  $ cd ../t
  $ hg -q update
  $ rm `hg status -un`
  $ template='{rev}:{node|short}  [{branch}]  {desc|firstline}\n'
  $ echo 'valuable new file' > b
  $ echo 'valuable modification' >> a
  $ hg commit -A -m'a valuable change'
  adding b
  $ hg update 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg rollback
  abort: rollback of last commit while not checked out may lose data
  (use -f to force)
  [255]
  $ hg tip -q
  2:4d9cd3795eea
  $ hg rollback -f
  repository tip rolled back to revision 1 (undo commit)
  $ hg status
  $ hg log --removed b   # yep, it's gone

same again, but emulate an old client that doesn't write undo.desc
  $ hg -q update
  $ echo 'valuable modification redux' >> a
  $ hg commit -m'a valuable change redux'
  $ rm .hg/undo.desc
  $ hg update 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg rollback
  rolling back unknown transaction
  $ cat a
  a

corrupt journal test
  $ echo "foo" > .hg/store/journal
  $ hg recover
  rolling back interrupted transaction
  couldn't read journal entry 'foo\n'!
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions

rollback disabled by config
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > rollback = false
  > EOF
  $ echo narf >> pinky-sayings.txt
  $ hg add pinky-sayings.txt
  $ hg ci -m 'First one.'
  $ hg rollback
  abort: rollback is disabled because it is unsafe
  (see `hg help -v rollback` for information)
  [255]

  $ cd ..

I/O errors on stdio are handled properly (issue5658)

  $ cat > badui.py << EOF
  > import errno
  > from mercurial.i18n import _
  > from mercurial import (
  >     error,
  >     ui as uimod,
  > )
  > 
  > def pretxncommit(ui, repo, **kwargs):
  >     ui.warn('warn during pretxncommit\n')
  > 
  > def pretxnclose(ui, repo, **kwargs):
  >     ui.warn('warn during pretxnclose\n')
  > 
  > def txnclose(ui, repo, **kwargs):
  >     ui.warn('warn during txnclose\n')
  > 
  > def txnabort(ui, repo, **kwargs):
  >     ui.warn('warn during abort\n')
  > 
  > class fdproxy(object):
  >     def __init__(self, ui, o):
  >         self._ui = ui
  >         self._o = o
  > 
  >     def __getattr__(self, attr):
  >         return getattr(self._o, attr)
  > 
  >     def write(self, msg):
  >         errors = set(self._ui.configlist('ui', 'ioerrors', []))
  >         pretxncommit = msg == 'warn during pretxncommit\n'
  >         pretxnclose = msg == 'warn during pretxnclose\n'
  >         txnclose = msg == 'warn during txnclose\n'
  >         txnabort = msg == 'warn during abort\n'
  >         msgabort = msg == _('transaction abort!\n')
  >         msgrollback = msg == _('rollback completed\n')
  > 
  >         if pretxncommit and 'pretxncommit' in errors:
  >             raise IOError(errno.EPIPE, 'simulated epipe')
  >         if pretxnclose and 'pretxnclose' in errors:
  >             raise IOError(errno.EIO, 'simulated eio')
  >         if txnclose and 'txnclose' in errors:
  >             raise IOError(errno.EBADF, 'simulated badf')
  >         if txnabort and 'txnabort' in errors:
  >             raise IOError(errno.EPIPE, 'simulated epipe')
  >         if msgabort and 'msgabort' in errors:
  >             raise IOError(errno.EBADF, 'simulated ebadf')
  >         if msgrollback and 'msgrollback' in errors:
  >             raise IOError(errno.EIO, 'simulated eio')
  > 
  >         return self._o.write(msg)
  > 
  > def uisetup(ui):
  >     class badui(ui.__class__):
  >         def write_err(self, *args, **kwargs):
  >             olderr = self.ferr
  >             try:
  >                 self.ferr = fdproxy(self, olderr)
  >                 return super(badui, self).write_err(*args, **kwargs)
  >             finally:
  >                 self.ferr = olderr
  > 
  >     ui.__class__ = badui
  > 
  > def reposetup(ui, repo):
  >     ui.setconfig('hooks', 'pretxnclose.badui', pretxnclose, 'badui')
  >     ui.setconfig('hooks', 'txnclose.badui', txnclose, 'badui')
  >     ui.setconfig('hooks', 'pretxncommit.badui', pretxncommit, 'badui')
  >     ui.setconfig('hooks', 'txnabort.badui', txnabort, 'badui')
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > badui = $TESTTMP/badui.py
  > EOF

An I/O error during pretxncommit is handled

  $ hg init ioerror-pretxncommit
  $ cd ioerror-pretxncommit
  $ echo 0 > foo
  $ hg -q commit -A -m initial
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose
  $ echo 1 > foo
  $ hg --config ui.ioerrors=pretxncommit commit -m 'error during pretxncommit'
  warn during pretxnclose
  warn during txnclose

  $ hg commit -m 'commit 1'
  nothing changed
  [1]

  $ cd ..

An I/O error during pretxnclose is handled

  $ hg init ioerror-pretxnclose
  $ cd ioerror-pretxnclose
  $ echo 0 > foo
  $ hg -q commit -A -m initial
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose

  $ echo 1 > foo
  $ hg --config ui.ioerrors=pretxnclose commit -m 'error during pretxnclose'
  warn during pretxncommit
  warn during txnclose

  $ hg commit -m 'commit 1'
  nothing changed
  [1]

  $ cd ..

An I/O error during txnclose is handled

  $ hg init ioerror-txnclose
  $ cd ioerror-txnclose
  $ echo 0 > foo
  $ hg -q commit -A -m initial
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose

  $ echo 1 > foo
  $ hg --config ui.ioerrors=txnclose commit -m 'error during txnclose'
  warn during pretxncommit
  warn during pretxnclose

  $ hg commit -m 'commit 1'
  nothing changed
  [1]

  $ cd ..

An I/O error writing "transaction abort" is handled

  $ hg init ioerror-msgabort
  $ cd ioerror-msgabort

  $ echo 0 > foo
  $ hg -q commit -A -m initial
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose

  $ echo 1 > foo
  $ hg --config ui.ioerrors=msgabort --config hooks.pretxncommit=false commit -m 'error during abort message'
  warn during abort
  rollback completed
  abort: pretxncommit hook exited with status 1
  [255]

  $ hg commit -m 'commit 1'
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose

  $ cd ..

An I/O error during txnabort should still result in rollback

  $ hg init ioerror-txnabort
  $ cd ioerror-txnabort

  $ echo 0 > foo
  $ hg -q commit -A -m initial
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose

  $ echo 1 > foo
  $ hg --config ui.ioerrors=txnabort --config hooks.pretxncommit=false commit -m 'error during abort'
  transaction abort!
  rollback completed
  abort: pretxncommit hook exited with status 1
  [255]

  $ hg commit -m 'commit 1'
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose

  $ cd ..

An I/O error writing "rollback completed" is handled

  $ hg init ioerror-msgrollback
  $ cd ioerror-msgrollback

  $ echo 0 > foo
  $ hg -q commit -A -m initial
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose

  $ echo 1 > foo

  $ hg --config ui.ioerrors=msgrollback --config hooks.pretxncommit=false commit -m 'error during rollback message'
  transaction abort!
  warn during abort
  abort: pretxncommit hook exited with status 1
  [255]

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

  $ cd ..

Multiple I/O errors after transaction open are handled.
This is effectively what happens if a peer disconnects in the middle
of a transaction.

  $ hg init ioerror-multiple
  $ cd ioerror-multiple
  $ echo 0 > foo
  $ hg -q commit -A -m initial
  warn during pretxncommit
  warn during pretxnclose
  warn during txnclose

  $ echo 1 > foo

  $ hg --config ui.ioerrors=pretxncommit,pretxnclose,txnclose,txnabort,msgabort,msgrollback commit -m 'multiple errors'

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions

  $ cd ..
