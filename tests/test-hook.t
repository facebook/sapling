commit hooks can see env vars
(and post-transaction one are run unlocked)

  $ cat << EOF >> $HGRCPATH
  > [experimental]
  > # drop me once bundle2 is the default,
  > # added to get test change early.
  > bundle2-exp = True
  > EOF

  $ cat > $TESTTMP/txnabort.checkargs.py <<EOF
  > def showargs(ui, repo, hooktype, **kwargs):
  >     ui.write('%s python hook: %s\n' % (hooktype, ','.join(sorted(kwargs))))
  > EOF

  $ hg init a
  $ cd a
  $ cat > .hg/hgrc <<EOF
  > [hooks]
  > commit = sh -c "HG_LOCAL= HG_TAG= printenv.py commit"
  > commit.b = sh -c "HG_LOCAL= HG_TAG= printenv.py commit.b"
  > precommit = sh -c  "HG_LOCAL= HG_NODE= HG_TAG= printenv.py precommit"
  > pretxncommit = sh -c "HG_LOCAL= HG_TAG= printenv.py pretxncommit"
  > pretxncommit.tip = hg -q tip
  > pre-identify = printenv.py pre-identify 1
  > pre-cat = printenv.py pre-cat
  > post-cat = printenv.py post-cat
  > pretxnopen = sh -c "HG_LOCAL= HG_TAG= printenv.py pretxnopen"
  > pretxnclose = sh -c "HG_LOCAL= HG_TAG= printenv.py pretxnclose"
  > txnclose = sh -c "HG_LOCAL= HG_TAG= printenv.py txnclose"
  > txnabort.0 = python:$TESTTMP/txnabort.checkargs.py:showargs
  > txnabort.1 = sh -c "HG_LOCAL= HG_TAG= printenv.py txnabort"
  > txnclose.checklock = sh -c "hg debuglock > /dev/null"
  > EOF
  $ echo a > a
  $ hg add a
  $ hg commit -m a
  precommit hook: HG_PARENT1=0000000000000000000000000000000000000000
  pretxnopen hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  pretxncommit hook: HG_NODE=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PARENT1=0000000000000000000000000000000000000000 HG_PENDING=$TESTTMP/a
  0:cb9a9f314b8b
  pretxnclose hook: HG_PENDING=$TESTTMP/a HG_PHASES_MOVED=1 HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  txnclose hook: HG_PHASES_MOVED=1 HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  commit hook: HG_NODE=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PARENT1=0000000000000000000000000000000000000000
  commit.b hook: HG_NODE=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PARENT1=0000000000000000000000000000000000000000

  $ hg clone . ../b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../b

changegroup hooks can see env vars

  $ cat > .hg/hgrc <<EOF
  > [hooks]
  > prechangegroup = printenv.py prechangegroup
  > changegroup = printenv.py changegroup
  > incoming = printenv.py incoming
  > EOF

pretxncommit and commit hooks can see both parents of merge

  $ cd ../a
  $ echo b >> a
  $ hg commit -m a1 -d "1 0"
  precommit hook: HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  pretxnopen hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  pretxncommit hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PENDING=$TESTTMP/a
  1:ab228980c14d
  pretxnclose hook: HG_PENDING=$TESTTMP/a HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  txnclose hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  commit hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  commit.b hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  $ hg update -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > b
  $ hg add b
  $ hg commit -m b -d '1 0'
  precommit hook: HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  pretxnopen hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  pretxncommit hook: HG_NODE=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b HG_PENDING=$TESTTMP/a
  2:ee9deb46ab31
  pretxnclose hook: HG_PENDING=$TESTTMP/a HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  txnclose hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  commit hook: HG_NODE=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  commit.b hook: HG_NODE=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT1=cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  created new head
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m merge -d '2 0'
  precommit hook: HG_PARENT1=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT2=ab228980c14deea8b9555d91c9581127383e40fd
  pretxnopen hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  pretxncommit hook: HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_PARENT1=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT2=ab228980c14deea8b9555d91c9581127383e40fd HG_PENDING=$TESTTMP/a
  3:07f3376c1e65
  pretxnclose hook: HG_PENDING=$TESTTMP/a HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  txnclose hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  commit hook: HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_PARENT1=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT2=ab228980c14deea8b9555d91c9581127383e40fd
  commit.b hook: HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_PARENT1=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_PARENT2=ab228980c14deea8b9555d91c9581127383e40fd

test generic hooks

  $ hg id
  pre-identify hook: HG_ARGS=id HG_OPTS={'bookmarks': None, 'branch': None, 'id': None, 'insecure': None, 'num': None, 'remotecmd': '', 'rev': '', 'ssh': '', 'tags': None} HG_PATS=[]
  abort: pre-identify hook exited with status 1
  [255]
  $ hg cat b
  pre-cat hook: HG_ARGS=cat b HG_OPTS={'decode': None, 'exclude': [], 'include': [], 'output': '', 'rev': ''} HG_PATS=['b']
  b
  post-cat hook: HG_ARGS=cat b HG_OPTS={'decode': None, 'exclude': [], 'include': [], 'output': '', 'rev': ''} HG_PATS=['b'] HG_RESULT=0

  $ cd ../b
  $ hg pull ../a
  pulling from ../a
  searching for changes
  prechangegroup hook: HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=file:$TESTTMP/a (glob)
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files
  changegroup hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=file:$TESTTMP/a (glob)
  incoming hook: HG_NODE=ab228980c14deea8b9555d91c9581127383e40fd HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=file:$TESTTMP/a (glob)
  incoming hook: HG_NODE=ee9deb46ab31e4cc3310f3cf0c3d668e4d8fffc2 HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=file:$TESTTMP/a (glob)
  incoming hook: HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=file:$TESTTMP/a (glob)
  (run 'hg update' to get a working copy)

tag hooks can see env vars

  $ cd ../a
  $ cat >> .hg/hgrc <<EOF
  > pretag = printenv.py pretag
  > tag = sh -c "HG_PARENT1= HG_PARENT2= printenv.py tag"
  > EOF
  $ hg tag -d '3 0' a
  pretag hook: HG_LOCAL=0 HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_TAG=a
  precommit hook: HG_PARENT1=07f3376c1e655977439df2a814e3cc14b27abac2
  pretxnopen hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  pretxncommit hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PARENT1=07f3376c1e655977439df2a814e3cc14b27abac2 HG_PENDING=$TESTTMP/a
  4:539e4b31b6dc
  pretxnclose hook: HG_PENDING=$TESTTMP/a HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  tag hook: HG_LOCAL=0 HG_NODE=07f3376c1e655977439df2a814e3cc14b27abac2 HG_TAG=a
  txnclose hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  commit hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PARENT1=07f3376c1e655977439df2a814e3cc14b27abac2
  commit.b hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PARENT1=07f3376c1e655977439df2a814e3cc14b27abac2
  $ hg tag -l la
  pretag hook: HG_LOCAL=1 HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_TAG=la
  tag hook: HG_LOCAL=1 HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_TAG=la

pretag hook can forbid tagging

  $ echo "pretag.forbid = printenv.py pretag.forbid 1" >> .hg/hgrc
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

  $ echo "pretxncommit.forbid0 = hg tip -q" >> .hg/hgrc
  $ echo "pretxncommit.forbid1 = printenv.py pretxncommit.forbid 1" >> .hg/hgrc
  $ echo z > z
  $ hg add z
  $ hg -q tip
  4:539e4b31b6dc
  $ hg commit -m 'fail' -d '4 0'
  precommit hook: HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10
  pretxnopen hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  pretxncommit hook: HG_NODE=6f611f8018c10e827fee6bd2bc807f937e761567 HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PENDING=$TESTTMP/a
  5:6f611f8018c1
  5:6f611f8018c1
  pretxncommit.forbid hook: HG_NODE=6f611f8018c10e827fee6bd2bc807f937e761567 HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PENDING=$TESTTMP/a
  transaction abort!
  txnabort python hook: txnid,txnname
  txnabort hook: HG_TXNID=TXN:* HG_TXNNAME=commit (glob)
  rollback completed
  abort: pretxncommit.forbid1 hook exited with status 1
  [255]
  $ hg -q tip
  4:539e4b31b6dc

(Check that no 'changelog.i.a' file were left behind)

  $ ls -1 .hg/store/
  00changelog.i
  00manifest.i
  data
  fncache
  journal.phaseroots
  phaseroots
  undo
  undo.backup.fncache
  undo.backupfiles
  undo.phaseroots


precommit hook can prevent commit

  $ echo "precommit.forbid = printenv.py precommit.forbid 1" >> .hg/hgrc
  $ hg commit -m 'fail' -d '4 0'
  precommit hook: HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10
  precommit.forbid hook: HG_PARENT1=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10
  abort: precommit.forbid hook exited with status 1
  [255]
  $ hg -q tip
  4:539e4b31b6dc

preupdate hook can prevent update

  $ echo "preupdate = printenv.py preupdate" >> .hg/hgrc
  $ hg update 1
  preupdate hook: HG_PARENT1=ab228980c14d
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved

update hook

  $ echo "update = printenv.py update" >> .hg/hgrc
  $ hg update
  preupdate hook: HG_PARENT1=539e4b31b6dc
  update hook: HG_ERROR=0 HG_PARENT1=539e4b31b6dc
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

pushkey hook

  $ echo "pushkey = printenv.py pushkey" >> .hg/hgrc
  $ cd ../b
  $ hg bookmark -r null foo
  $ hg push -B foo ../a
  pushing to ../a
  searching for changes
  no changes found
  pretxnopen hook: HG_TXNID=TXN:* HG_TXNNAME=push (glob)
  pretxnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_PENDING=$TESTTMP/a HG_SOURCE=push HG_TXNID=TXN:* HG_TXNNAME=push HG_URL=push (glob)
  pushkey hook: HG_KEY=foo HG_NAMESPACE=bookmarks HG_NEW=0000000000000000000000000000000000000000 HG_RET=1
  txnclose hook: HG_BOOKMARK_MOVED=1 HG_BUNDLE2=1 HG_SOURCE=push HG_TXNID=TXN:* HG_TXNNAME=push HG_URL=push (glob)
  exporting bookmark foo
  [1]
  $ cd ../a

listkeys hook

  $ echo "listkeys = printenv.py listkeys" >> .hg/hgrc
  $ hg bookmark -r null bar
  $ cd ../b
  $ hg pull -B bar ../a
  pulling from ../a
  listkeys hook: HG_NAMESPACE=bookmarks HG_VALUES={'bar': '0000000000000000000000000000000000000000', 'foo': '0000000000000000000000000000000000000000'}
  no changes found
  listkeys hook: HG_NAMESPACE=phase HG_VALUES={}
  adding remote bookmark bar
  listkeys hook: HG_NAMESPACE=phases HG_VALUES={'cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b': '1', 'publishing': 'True'}
  $ cd ../a

test that prepushkey can prevent incoming keys

  $ echo "prepushkey = printenv.py prepushkey.forbid 1" >> .hg/hgrc
  $ cd ../b
  $ hg bookmark -r null baz
  $ hg push -B baz ../a
  pushing to ../a
  searching for changes
  listkeys hook: HG_NAMESPACE=phases HG_VALUES={'cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b': '1', 'publishing': 'True'}
  listkeys hook: HG_NAMESPACE=bookmarks HG_VALUES={'bar': '0000000000000000000000000000000000000000', 'foo': '0000000000000000000000000000000000000000'}
  no changes found
  pretxnopen hook: HG_TXNID=TXN:* HG_TXNNAME=push (glob)
  prepushkey.forbid hook: HG_BUNDLE2=1 HG_KEY=baz HG_NAMESPACE=bookmarks HG_NEW=0000000000000000000000000000000000000000 HG_SOURCE=push HG_TXNID=TXN:* HG_URL=push (glob)
  pushkey-abort: prepushkey hook exited with status 1
  pretxnclose hook: HG_BUNDLE2=1 HG_SOURCE=push HG_TXNID=TXN:* HG_TXNNAME=push HG_URL=push (glob)
  txnclose hook: HG_BUNDLE2=1 HG_SOURCE=push HG_TXNID=TXN:* HG_TXNNAME=push HG_URL=push (glob)
  exporting bookmark baz failed!
  listkeys hook: HG_NAMESPACE=phases HG_VALUES={'cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b': '1', 'publishing': 'True'}
  [1]
  $ cd ../a

test that prelistkeys can prevent listing keys

  $ echo "prelistkeys = printenv.py prelistkeys.forbid 1" >> .hg/hgrc
  $ hg bookmark -r null quux
  $ cd ../b
  $ hg pull -B quux ../a
  pulling from ../a
  prelistkeys.forbid hook: HG_NAMESPACE=bookmarks
  abort: prelistkeys hook exited with status 1
  [255]
  $ cd ../a
  $ rm .hg/hgrc

prechangegroup hook can prevent incoming changes

  $ cd ../b
  $ hg -q tip
  3:07f3376c1e65
  $ cat > .hg/hgrc <<EOF
  > [hooks]
  > prechangegroup.forbid = printenv.py prechangegroup.forbid 1
  > EOF
  $ hg pull ../a
  pulling from ../a
  searching for changes
  prechangegroup.forbid hook: HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=file:$TESTTMP/a (glob)
  abort: prechangegroup.forbid hook exited with status 1
  [255]

pretxnchangegroup hook can see incoming changes, can roll back txn,
incoming changes no longer there after

  $ cat > .hg/hgrc <<EOF
  > [hooks]
  > pretxnchangegroup.forbid0 = hg tip -q
  > pretxnchangegroup.forbid1 = printenv.py pretxnchangegroup.forbid 1
  > EOF
  $ hg pull ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  4:539e4b31b6dc
  pretxnchangegroup.forbid hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_PENDING=$TESTTMP/b HG_SOURCE=pull HG_TXNID=TXN:* HG_URL=file:$TESTTMP/a (glob)
  transaction abort!
  rollback completed
  abort: pretxnchangegroup.forbid1 hook exited with status 1
  [255]
  $ hg -q tip
  3:07f3376c1e65

outgoing hooks can see env vars

  $ rm .hg/hgrc
  $ cat > ../a/.hg/hgrc <<EOF
  > [hooks]
  > preoutgoing = printenv.py preoutgoing
  > outgoing = printenv.py outgoing
  > EOF
  $ hg pull ../a
  pulling from ../a
  searching for changes
  preoutgoing hook: HG_SOURCE=pull
  outgoing hook: HG_NODE=539e4b31b6dc99b3cfbaa6b53cbc1c1f9a1e3a10 HG_SOURCE=pull
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark quux
  (run 'hg update' to get a working copy)
  $ hg rollback
  repository tip rolled back to revision 3 (undo pull)

preoutgoing hook can prevent outgoing changes

  $ echo "preoutgoing.forbid = printenv.py preoutgoing.forbid 1" >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  preoutgoing hook: HG_SOURCE=pull
  preoutgoing.forbid hook: HG_SOURCE=pull
  abort: preoutgoing.forbid hook exited with status 1
  [255]

outgoing hooks work for local clones

  $ cd ..
  $ cat > a/.hg/hgrc <<EOF
  > [hooks]
  > preoutgoing = printenv.py preoutgoing
  > outgoing = printenv.py outgoing
  > EOF
  $ hg clone a c
  preoutgoing hook: HG_SOURCE=clone
  outgoing hook: HG_NODE=0000000000000000000000000000000000000000 HG_SOURCE=clone
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf c

preoutgoing hook can prevent outgoing changes for local clones

  $ echo "preoutgoing.forbid = printenv.py preoutgoing.forbid 1" >> a/.hg/hgrc
  $ hg clone a zzz
  preoutgoing hook: HG_SOURCE=clone
  preoutgoing.forbid hook: HG_SOURCE=clone
  abort: preoutgoing.forbid hook exited with status 1
  [255]

  $ cd "$TESTTMP/b"

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
  > def printtags(ui, repo, **args):
  >     print sorted(repo.tags())
  > 
  > class container:
  >     unreachable = 1
  > EOF

test python hooks

#if windows
  $ PYTHONPATH="$TESTTMP/b;$PYTHONPATH"
#else
  $ PYTHONPATH="$TESTTMP/b:$PYTHONPATH"
#endif
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
  adding remote bookmark quux
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
  committing files:
  foo
  committing manifest
  committing changelog
  calling hook commit.auto: hgext_hookext.autohook
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

  $ echo '[hooks]' > .hg/hgrc
  $ echo "update.ne = python:`pwd`/nonexistent.py:testhook" >> .hg/hgrc
  $ echo "pre-identify.npmd = python:`pwd`/:no_python_module_dir" >> .hg/hgrc

  $ hg up null
  loading update.ne hook failed:
  abort: No such file or directory: $TESTTMP/d/repo/nonexistent.py
  [255]

  $ hg id
  loading pre-identify.npmd hook failed:
  abort: No module named repo!
  [255]

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
  $ hg --traceback commit -ma 2>&1 | egrep -v '^( +File|    [a-zA-Z(])'
  exception from first failed import attempt:
  Traceback (most recent call last):
  ImportError: No module named somebogusmodule
  exception from second failed import attempt:
  Traceback (most recent call last):
  ImportError: No module named hgext_importfail
  Traceback (most recent call last):
  Abort: precommit.importfail hook is invalid (import of "importfail" failed)
  abort: precommit.importfail hook is invalid (import of "importfail" failed)

Issue1827: Hooks Update & Commit not completely post operation

commit and update hooks should run after command completion.  The largefiles
use demonstrates a recursive wlock, showing the hook doesn't run until the
final release (and dirstate flush).

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'commit = hg id' >> .hg/hgrc
  $ echo 'update = hg id' >> .hg/hgrc
  $ echo bb > a
  $ hg ci -ma
  223eafe2750c tip
  $ hg up 0 --config extensions.largefiles=
  cb9a9f314b8b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

make sure --verbose (and --quiet/--debug etc.) are propagated to the local ui
that is passed to pre/post hooks

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'pre-identify = python:hooktests.verbosehook' >> .hg/hgrc
  $ hg id
  cb9a9f314b8b
  $ hg id --verbose
  calling hook pre-identify: hooktests.verbosehook
  verbose output from hook
  cb9a9f314b8b

Ensure hooks can be prioritized

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'pre-identify.a = python:hooktests.verbosehook' >> .hg/hgrc
  $ echo 'pre-identify.b = python:hooktests.verbosehook' >> .hg/hgrc
  $ echo 'priority.pre-identify.b = 1' >> .hg/hgrc
  $ echo 'pre-identify.c = python:hooktests.verbosehook' >> .hg/hgrc
  $ hg id --verbose
  calling hook pre-identify.b: hooktests.verbosehook
  verbose output from hook
  calling hook pre-identify.a: hooktests.verbosehook
  verbose output from hook
  calling hook pre-identify.c: hooktests.verbosehook
  verbose output from hook
  cb9a9f314b8b

new tags must be visible in pretxncommit (issue3210)

  $ echo 'pretxncommit.printtags = python:hooktests.printtags' >> .hg/hgrc
  $ hg tag -f foo
  ['a', 'foo', 'tip']

new commits must be visible in pretxnchangegroup (issue3428)

  $ cd ..
  $ hg init to
  $ echo '[hooks]' >> to/.hg/hgrc
  $ echo 'pretxnchangegroup = hg --traceback tip' >> to/.hg/hgrc
  $ echo a >> to/a
  $ hg --cwd to ci -Ama
  adding a
  $ hg clone to from
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aa >> from/a
  $ hg --cwd from ci -mb
  $ hg --cwd from push
  pushing to $TESTTMP/to (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  changeset:   1:9836a07b9b9d
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
