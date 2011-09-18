commit hooks can see env vars

  $ hg init a
  $ cd a
  $ echo "[hooks]" > .hg/hgrc
  $ echo 'commit = unset HG_LOCAL HG_TAG; python "$TESTDIR"/printenv.py commit' >> .hg/hgrc
  $ echo 'commit.b = unset HG_LOCAL HG_TAG; python "$TESTDIR"/printenv.py commit.b' >> .hg/hgrc
  $ echo 'precommit = unset HG_LOCAL HG_NODE HG_TAG; python "$TESTDIR"/printenv.py precommit' >> .hg/hgrc
  $ echo 'pretxncommit = unset HG_LOCAL HG_TAG; python "$TESTDIR"/printenv.py pretxncommit' >> .hg/hgrc
  $ echo 'pretxncommit.tip = hg -q tip' >> .hg/hgrc
  $ echo 'pre-identify = python "$TESTDIR"/printenv.py pre-identify 1' >> .hg/hgrc
  $ echo 'pre-cat = python "$TESTDIR"/printenv.py pre-cat' >> .hg/hgrc
  $ echo 'post-cat = python "$TESTDIR"/printenv.py post-cat' >> .hg/hgrc
  $ echo a > a
  $ hg add a
  $ hg commit -m a
  precommit hook: HG_PARENT1=0000000000000000000000000000000000000000 
  pretxncommit hook: HG_NODE=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PARENT1=0000000000000000000000000000000000000000 HG_PENDING=$TESTTMP/a 
  0:cb9a9f314b8b
  commit hook: HG_NODE=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PARENT1=0000000000000000000000000000000000000000 
  commit.b hook: HG_NODE=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PARENT1=0000000000000000000000000000000000000000 

  $ hg clone . ../b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../b

changegroup hooks can see env vars

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'prechangegroup = python "$TESTDIR"/printenv.py prechangegroup' >> .hg/hgrc
  $ echo 'changegroup = python "$TESTDIR"/printenv.py changegroup' >> .hg/hgrc
  $ echo 'incoming = python "$TESTDIR"/printenv.py incoming' >> .hg/hgrc

pretxncommit and commit hooks can see both parents of merge

  $ cd ../a
  $ echo b >> a
  $ hg commit -m a1 -d "1 0"
  precommit hook: HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 
  pretxncommit hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PENDING=$TESTTMP/a 
  1:ab228980c14d
  commit hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 
  commit.b hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 
  $ hg update -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > b
  $ hg add b
  $ hg commit -m b -d '1 0'
  precommit hook: HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 
  pretxncommit hook: HG_NODE=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PENDING=$TESTTMP/a 
  2:ee9deb46ab31
  commit hook: HG_NODE=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 
  commit.b hook: HG_NODE=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 
  created new head
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m merge -d '2 0'
  precommit hook: HG_PARENT1=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT2=ab228980c14deea8b9555d91c9581127383e40fd 
  pretxncommit hook: HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_PARENT1=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT2=ab228980c14deea8b9555d91c9581127383e40fd HG_PENDING=$TESTTMP/a 
  3:07f3376c1e65
  commit hook: HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_PARENT1=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT2=ab228980c14deea8b9555d91c9581127383e40fd 
  commit.b hook: HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_PARENT1=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT2=ab228980c14deea8b9555d91c9581127383e40fd 

test generic hooks

  $ hg id
  pre-identify hook: HG_ARGS=id HG_OPTS={'bookmarks': None, 'branch': None, 'id': None, 'num': None, 'rev': '', 'tags': None} HG_PATS=[] 
  warning: pre-identify hook exited with status 1
  [1]
  $ hg cat b
  pre-cat hook: HG_ARGS=cat b HG_OPTS={'decode': None, 'exclude': [], 'include': [], 'output': '', 'rev': ''} HG_PATS=['b'] 
  b
  post-cat hook: HG_ARGS=cat b HG_OPTS={'decode': None, 'exclude': [], 'include': [], 'output': '', 'rev': ''} HG_PATS=['b'] HG_RESULT=0 

  $ cd ../b
  $ hg pull ../a
  pulling from ../a
  searching for changes
  prechangegroup hook: HG_SOURCE=pull HG_URL=file:$TESTTMP/a 
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files
  changegroup hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_SOURCE=pull HG_URL=file:$TESTTMP/a 
  incoming hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_SOURCE=pull HG_URL=file:$TESTTMP/a 
  incoming hook: HG_NODE=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_SOURCE=pull HG_URL=file:$TESTTMP/a 
  incoming hook: HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_SOURCE=pull HG_URL=file:$TESTTMP/a 
  (run 'hg update' to get a working copy)

tag hooks can see env vars

  $ cd ../a
  $ echo 'pretag = python "$TESTDIR"/printenv.py pretag' >> .hg/hgrc
  $ echo 'tag = unset HG_PARENT1 HG_PARENT2; python "$TESTDIR"/printenv.py tag' >> .hg/hgrc
  $ hg tag -d '3 0' a
  pretag hook: HG_LOCAL=0 HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_TAG=a 
  precommit hook: HG_PARENT1=07f3376c1e655977439df2a814e3cc14b27abac2 
  pretxncommit hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PARENT1=07f3376c1e655977439df2a814e3cc14b27abac2 HG_PENDING=$TESTTMP/a 
  4:539e4b31b6dc
  commit hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PARENT1=07f3376c1e655977439df2a814e3cc14b27abac2 
  commit.b hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PARENT1=07f3376c1e655977439df2a814e3cc14b27abac2 
  tag hook: HG_LOCAL=0 HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_TAG=a 
  $ hg tag -l la
  pretag hook: HG_LOCAL=1 HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_TAG=la 
  tag hook: HG_LOCAL=1 HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_TAG=la 

pretag hook can forbid tagging

  $ echo 'pretag.forbid = python "$TESTDIR"/printenv.py pretag.forbid 1' >> .hg/hgrc
  $ hg tag -d '4 0' fa
  pretag hook: HG_LOCAL=0 HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_TAG=fa 
  pretag.forbid hook: HG_LOCAL=0 HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_TAG=fa 
  abort: pretag.forbid hook exited with status 1
  [255]
  $ hg tag -l fla
  pretag hook: HG_LOCAL=1 HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_TAG=fla 
  pretag.forbid hook: HG_LOCAL=1 HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_TAG=fla 
  abort: pretag.forbid hook exited with status 1
  [255]

pretxncommit hook can see changeset, can roll back txn, changeset no
more there after

  $ echo 'pretxncommit.forbid0 = hg tip -q' >> .hg/hgrc
  $ echo 'pretxncommit.forbid1 = python "$TESTDIR"/printenv.py pretxncommit.forbid 1' >> .hg/hgrc
  $ echo z > z
  $ hg add z
  $ hg -q tip
  4:539e4b31b6dc
  $ hg commit -m 'fail' -d '4 0'
  precommit hook: HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 
  pretxncommit hook: HG_NODE=6f611f8018c10e827fee6bd2bc807f937e761567 HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PENDING=$TESTTMP/a 
  5:6f611f8018c1
  5:6f611f8018c1
  pretxncommit.forbid hook: HG_NODE=6f611f8018c10e827fee6bd2bc807f937e761567 HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PENDING=$TESTTMP/a 
  transaction abort!
  rollback completed
  abort: pretxncommit.forbid1 hook exited with status 1
  [255]
  $ hg -q tip
  4:539e4b31b6dc

precommit hook can prevent commit

  $ echo 'precommit.forbid = python "$TESTDIR"/printenv.py precommit.forbid 1' >> .hg/hgrc
  $ hg commit -m 'fail' -d '4 0'
  precommit hook: HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 
  precommit.forbid hook: HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 
  abort: precommit.forbid hook exited with status 1
  [255]
  $ hg -q tip
  4:539e4b31b6dc

preupdate hook can prevent update

  $ echo 'preupdate = python "$TESTDIR"/printenv.py preupdate' >> .hg/hgrc
  $ hg update 1
  preupdate hook: HG_PARENT1=ab228980c14d 
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved

update hook

  $ echo 'update = python "$TESTDIR"/printenv.py update' >> .hg/hgrc
  $ hg update
  preupdate hook: HG_PARENT1=539e4b31b6dc 
  update hook: HG_ERROR=0 HG_PARENT1=539e4b31b6dc 
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

pushkey hook

  $ echo 'pushkey = python "$TESTDIR"/printenv.py pushkey' >> .hg/hgrc
  $ cd ../b
  $ hg bookmark -r null foo
  $ hg push -B foo ../a
  pushing to ../a
  searching for changes
  no changes found
  exporting bookmark foo
  pushkey hook: HG_KEY=foo HG_NAMESPACE=bookmarks HG_NEW=0000000000000000000000000000000000000000 HG_RET=1 
  $ cd ../a

listkeys hook

  $ echo 'listkeys = python "$TESTDIR"/printenv.py listkeys' >> .hg/hgrc
  $ hg bookmark -r null bar
  $ cd ../b
  $ hg pull -B bar ../a
  pulling from ../a
  listkeys hook: HG_NAMESPACE=bookmarks HG_VALUES={'bar': '0000000000000000000000000000000000000000', 'foo': '0000000000000000000000000000000000000000'} 
  no changes found
  listkeys hook: HG_NAMESPACE=bookmarks HG_VALUES={'bar': '0000000000000000000000000000000000000000', 'foo': '0000000000000000000000000000000000000000'} 
  importing bookmark bar
  $ cd ../a

test that prepushkey can prevent incoming keys

  $ echo 'prepushkey = python "$TESTDIR"/printenv.py prepushkey.forbid 1' >> .hg/hgrc
  $ cd ../b
  $ hg bookmark -r null baz
  $ hg push -B baz ../a
  pushing to ../a
  searching for changes
  no changes found
  listkeys hook: HG_NAMESPACE=bookmarks HG_VALUES={'bar': '0000000000000000000000000000000000000000', 'foo': '0000000000000000000000000000000000000000'} 
  listkeys hook: HG_NAMESPACE=bookmarks HG_VALUES={'bar': '0000000000000000000000000000000000000000', 'foo': '0000000000000000000000000000000000000000'} 
  exporting bookmark baz
  prepushkey.forbid hook: HG_KEY=baz HG_NAMESPACE=bookmarks HG_NEW=0000000000000000000000000000000000000000 
  abort: prepushkey hook exited with status 1
  [255]
  $ cd ../a

test that prelistkeys can prevent listing keys

  $ echo 'prelistkeys = python "$TESTDIR"/printenv.py prelistkeys.forbid 1' >> .hg/hgrc
  $ hg bookmark -r null quux
  $ cd ../b
  $ hg pull -B quux ../a
  pulling from ../a
  prelistkeys.forbid hook: HG_NAMESPACE=bookmarks 
  abort: prelistkeys hook exited with status 1
  [255]
  $ cd ../a

prechangegroup hook can prevent incoming changes

  $ cd ../b
  $ hg -q tip
  3:07f3376c1e65
  $ echo '[hooks]' > .hg/hgrc
  $ echo 'prechangegroup.forbid = python "$TESTDIR"/printenv.py prechangegroup.forbid 1' >> .hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  prechangegroup.forbid hook: HG_SOURCE=pull HG_URL=file:$TESTTMP/a 
  abort: prechangegroup.forbid hook exited with status 1
  [255]

pretxnchangegroup hook can see incoming changes, can roll back txn,
incoming changes no longer there after

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'pretxnchangegroup.forbid0 = hg tip -q' >> .hg/hgrc
  $ echo 'pretxnchangegroup.forbid1 = python "$TESTDIR"/printenv.py pretxnchangegroup.forbid 1' >> .hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  4:539e4b31b6dc
  pretxnchangegroup.forbid hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PENDING=$TESTTMP/b HG_SOURCE=pull HG_URL=file:$TESTTMP/a 
  transaction abort!
  rollback completed
  abort: pretxnchangegroup.forbid1 hook exited with status 1
  [255]
  $ hg -q tip
  3:07f3376c1e65

outgoing hooks can see env vars

  $ rm .hg/hgrc
  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing = python "$TESTDIR"/printenv.py preoutgoing' >> ../a/.hg/hgrc
  $ echo 'outgoing = python "$TESTDIR"/printenv.py outgoing' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  preoutgoing hook: HG_SOURCE=pull 
  adding changesets
  outgoing hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_SOURCE=pull 
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg rollback
  repository tip rolled back to revision 3 (undo pull)

preoutgoing hook can prevent outgoing changes

  $ echo 'preoutgoing.forbid = python "$TESTDIR"/printenv.py preoutgoing.forbid 1' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  preoutgoing hook: HG_SOURCE=pull 
  preoutgoing.forbid hook: HG_SOURCE=pull 
  abort: preoutgoing.forbid hook exited with status 1
  [255]

outgoing hooks work for local clones

  $ cd ..
  $ echo '[hooks]' > a/.hg/hgrc
  $ echo 'preoutgoing = python "$TESTDIR"/printenv.py preoutgoing' >> a/.hg/hgrc
  $ echo 'outgoing = python "$TESTDIR"/printenv.py outgoing' >> a/.hg/hgrc
  $ hg clone a c
  preoutgoing hook: HG_SOURCE=clone 
  outgoing hook: HG_NODE=0000000000000000000000000000000000000000 HG_SOURCE=clone 
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf c

preoutgoing hook can prevent outgoing changes for local clones

  $ echo 'preoutgoing.forbid = python "$TESTDIR"/printenv.py preoutgoing.forbid 1' >> a/.hg/hgrc
  $ hg clone a zzz
  preoutgoing hook: HG_SOURCE=clone 
  preoutgoing.forbid hook: HG_SOURCE=clone 
  abort: preoutgoing.forbid hook exited with status 1
  [255]
  $ cd b

  $ cat > hooktests.py <<EOF
  > from mercurial import util
  > 
  > uncallable = 0
  > 
  > def printargs(args):
  >     args.pop('ui', None)
  >     args.pop('repo', None)
  >     a = list(args.items())
  >     a.sort()
  >     print 'hook args:'
  >     for k, v in a:
  >        print ' ', k, v
  > 
  > def passhook(**args):
  >     printargs(args)
  > 
  > def failhook(**args):
  >     printargs(args)
  >     return True
  > 
  > class LocalException(Exception):
  >     pass
  > 
  > def raisehook(**args):
  >     raise LocalException('exception from hook')
  > 
  > def aborthook(**args):
  >     raise util.Abort('raise abort from hook')
  > 
  > def brokenhook(**args):
  >     return 1 + {}
  > 
  > def verbosehook(ui, **args):
  >     ui.note('verbose output from hook\n')
  > 
  > class container:
  >     unreachable = 1
  > EOF

test python hooks

  $ PYTHONPATH="`pwd`:$PYTHONPATH"
  $ export PYTHONPATH

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.broken = python:hooktests.brokenhook' >> ../a/.hg/hgrc
  $ hg pull ../a 2>&1 | grep 'raised an exception'
  error: preoutgoing.broken hook raised an exception: unsupported operand type(s) for +: 'int' and 'dict'

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.raise = python:hooktests.raisehook' >> ../a/.hg/hgrc
  $ hg pull ../a 2>&1 | grep 'raised an exception'
  error: preoutgoing.raise hook raised an exception: exception from hook

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.abort = python:hooktests.aborthook' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  error: preoutgoing.abort hook failed: raise abort from hook
  abort: raise abort from hook
  [255]

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.fail = python:hooktests.failhook' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  hook args:
    hooktype preoutgoing
    source pull
  abort: preoutgoing.fail hook failed
  [255]

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.uncallable = python:hooktests.uncallable' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.uncallable hook is invalid ("hooktests.uncallable" is not callable)
  [255]

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.nohook = python:hooktests.nohook' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.nohook hook is invalid ("hooktests.nohook" is not defined)
  [255]

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.nomodule = python:nomodule' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.nomodule hook is invalid ("nomodule" not in a module)
  [255]

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.badmodule = python:nomodule.nowhere' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.badmodule hook is invalid (import of "nomodule" failed)
  [255]

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.unreachable = python:hooktests.container.unreachable' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.unreachable hook is invalid (import of "hooktests.container" failed)
  [255]

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.pass = python:hooktests.passhook' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  hook args:
    hooktype preoutgoing
    source pull
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

make sure --traceback works

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'commit.abort = python:hooktests.aborthook' >> .hg/hgrc

  $ echo aa > a
  $ hg --traceback commit -d '0 0' -ma 2>&1 | grep '^Traceback'
  Traceback (most recent call last):

  $ cd ..
  $ hg init c
  $ cd c

  $ cat > hookext.py <<EOF
  > def autohook(**args):
  >     print "Automatically installed hook"
  > 
  > def reposetup(ui, repo):
  >     repo.ui.setconfig("hooks", "commit.auto", autohook)
  > EOF
  $ echo '[extensions]' >> .hg/hgrc
  $ echo 'hookext = hookext.py' >> .hg/hgrc

  $ touch foo
  $ hg add foo
  $ hg ci -d '0 0' -m 'add foo'
  Automatically installed hook
  $ echo >> foo
  $ hg ci --debug -d '0 0' -m 'change foo'
  foo
  calling hook commit.auto: <function autohook at *> (glob)
  Automatically installed hook
  committed changeset 1:52998019f6252a2b893452765fcb0a47351a5708

  $ hg showconfig hooks
  hooks.commit.auto=<function autohook at *> (glob)

test python hook configured with python:[file]:[hook] syntax

  $ cd ..
  $ mkdir d
  $ cd d
  $ hg init repo
  $ mkdir hooks

  $ cd hooks
  $ cat > testhooks.py <<EOF
  > def testhook(**args):
  >     print 'hook works'
  > EOF
  $ echo '[hooks]' > ../repo/.hg/hgrc
  $ echo "pre-commit.test = python:`pwd`/testhooks.py:testhook" >> ../repo/.hg/hgrc

  $ cd ../repo
  $ hg commit -d '0 0'
  hook works
  nothing changed
  [1]

  $ cd ../../b

make sure --traceback works on hook import failure

  $ cat > importfail.py <<EOF
  > import somebogusmodule
  > # dereference something in the module to force demandimport to load it
  > somebogusmodule.whatever
  > EOF

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'precommit.importfail = python:importfail.whatever' >> .hg/hgrc

  $ echo a >> a
  $ hg --traceback commit -ma 2>&1 | egrep '^(exception|Traceback|ImportError)'
  exception from first failed import attempt:
  Traceback (most recent call last):
  ImportError: No module named somebogusmodule
  exception from second failed import attempt:
  Traceback (most recent call last):
  ImportError: No module named hgext_importfail
  Traceback (most recent call last):

Issue1827: Hooks Update & Commit not completely post operation

commit and update hooks should run after command completion

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'commit = hg id' >> .hg/hgrc
  $ echo 'update = hg id' >> .hg/hgrc
  $ echo bb > a
  $ hg ci -ma
  223eafe2750c tip
  $ hg up 0
  cb9a9f314b8b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

make sure --verbose (and --quiet/--debug etc.) are propogated to the local ui
that is passed to pre/post hooks

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'pre-identify = python:hooktests.verbosehook' >> .hg/hgrc
  $ hg id
  cb9a9f314b8b
  $ hg id --verbose
  calling hook pre-identify: hooktests.verbosehook
  verbose output from hook
  cb9a9f314b8b
