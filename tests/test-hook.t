  $ cp "$TESTDIR"/printenv.py .

# commit hooks can see env vars

  $ hg init a
  $ cd a
  $ echo "[hooks]" > .hg/hgrc
  $ echo 'commit = unset HG_LOCAL HG_TAG; python ../printenv.py commit' >> .hg/hgrc
  $ echo 'commit.b = unset HG_LOCAL HG_TAG; python ../printenv.py commit.b' >> .hg/hgrc
  $ echo 'precommit = unset HG_LOCAL HG_NODE HG_TAG; python ../printenv.py precommit' >> .hg/hgrc
  $ echo 'pretxncommit = unset HG_LOCAL HG_TAG; python ../printenv.py pretxncommit' >> .hg/hgrc
  $ echo 'pretxncommit.tip = hg -q tip' >> .hg/hgrc
  $ echo 'pre-identify = python ../printenv.py pre-identify 1' >> .hg/hgrc
  $ echo 'pre-cat = python ../printenv.py pre-cat' >> .hg/hgrc
  $ echo 'post-cat = python ../printenv.py post-cat' >> .hg/hgrc
  $ echo a > a
  $ hg add a
  $ hg commit -m a -d "1000000 0"
  precommit hook: HG_PARENT1=0000000000000000000000000000000000000000 
  pretxncommit hook: HG_NODE=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b HG_PARENT1=0000000000000000000000000000000000000000 HG_PENDING=$HGTMP/test-hook.t/a 
  0:29b62aeb769f
  commit hook: HG_NODE=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b HG_PARENT1=0000000000000000000000000000000000000000 
  commit.b hook: HG_NODE=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b HG_PARENT1=0000000000000000000000000000000000000000 

  $ hg clone . ../b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../b

# changegroup hooks can see env vars

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'prechangegroup = python ../printenv.py prechangegroup' >> .hg/hgrc
  $ echo 'changegroup = python ../printenv.py changegroup' >> .hg/hgrc
  $ echo 'incoming = python ../printenv.py incoming' >> .hg/hgrc

# pretxncommit and commit hooks can see both parents of merge

  $ cd ../a
  $ echo b >> a
  $ hg commit -m a1 -d "1 0"
  precommit hook: HG_PARENT1=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b 
  pretxncommit hook: HG_NODE=b702efe9688826e3a91283852b328b84dbf37bc2 HG_PARENT1=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b HG_PENDING=$HGTMP/test-hook.t/a 
  1:b702efe96888
  commit hook: HG_NODE=b702efe9688826e3a91283852b328b84dbf37bc2 HG_PARENT1=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b 
  commit.b hook: HG_NODE=b702efe9688826e3a91283852b328b84dbf37bc2 HG_PARENT1=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b 
  $ hg update -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > b
  $ hg add b
  $ hg commit -m b -d '1 0'
  precommit hook: HG_PARENT1=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b 
  pretxncommit hook: HG_NODE=1324a5531bac09b329c3845d35ae6a7526874edb HG_PARENT1=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b HG_PENDING=$HGTMP/test-hook.t/a 
  2:1324a5531bac
  commit hook: HG_NODE=1324a5531bac09b329c3845d35ae6a7526874edb HG_PARENT1=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b 
  commit.b hook: HG_NODE=1324a5531bac09b329c3845d35ae6a7526874edb HG_PARENT1=29b62aeb769fdf78d8d9c5f28b017f76d7ef824b 
  created new head
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m merge -d '2 0'
  precommit hook: HG_PARENT1=1324a5531bac09b329c3845d35ae6a7526874edb HG_PARENT2=b702efe9688826e3a91283852b328b84dbf37bc2 
  pretxncommit hook: HG_NODE=4c52fb2e402287dd5dc052090682536c8406c321 HG_PARENT1=1324a5531bac09b329c3845d35ae6a7526874edb HG_PARENT2=b702efe9688826e3a91283852b328b84dbf37bc2 HG_PENDING=$HGTMP/test-hook.t/a 
  3:4c52fb2e4022
  commit hook: HG_NODE=4c52fb2e402287dd5dc052090682536c8406c321 HG_PARENT1=1324a5531bac09b329c3845d35ae6a7526874edb HG_PARENT2=b702efe9688826e3a91283852b328b84dbf37bc2 
  commit.b hook: HG_NODE=4c52fb2e402287dd5dc052090682536c8406c321 HG_PARENT1=1324a5531bac09b329c3845d35ae6a7526874edb HG_PARENT2=b702efe9688826e3a91283852b328b84dbf37bc2 

# test generic hooks

  $ hg id
  pre-identify hook: HG_ARGS=id HG_OPTS={'tags': None, 'rev': '', 'num': None, 'branch': None, 'id': None} HG_PATS=[] 
  warning: pre-identify hook exited with status 1
  $ hg cat b
  pre-cat hook: HG_ARGS=cat b HG_OPTS={'rev': '', 'decode': None, 'exclude': [], 'output': '', 'include': []} HG_PATS=['b'] 
  post-cat hook: HG_ARGS=cat b HG_OPTS={'rev': '', 'decode': None, 'exclude': [], 'output': '', 'include': []} HG_PATS=['b'] HG_RESULT=0 
  b

  $ cd ../b
  $ hg pull ../a
  prechangegroup hook: HG_SOURCE=pull HG_URL=file: 
  changegroup hook: HG_NODE=b702efe9688826e3a91283852b328b84dbf37bc2 HG_SOURCE=pull HG_URL=file: 
  incoming hook: HG_NODE=b702efe9688826e3a91283852b328b84dbf37bc2 HG_SOURCE=pull HG_URL=file: 
  incoming hook: HG_NODE=1324a5531bac09b329c3845d35ae6a7526874edb HG_SOURCE=pull HG_URL=file: 
  incoming hook: HG_NODE=4c52fb2e402287dd5dc052090682536c8406c321 HG_SOURCE=pull HG_URL=file: 
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

# tag hooks can see env vars

  $ cd ../a
  $ echo 'pretag = python ../printenv.py pretag' >> .hg/hgrc
  $ echo 'tag = unset HG_PARENT1 HG_PARENT2; python ../printenv.py tag' >> .hg/hgrc
  $ hg tag -d '3 0' a
  pretag hook: HG_LOCAL=0 HG_NODE=4c52fb2e402287dd5dc052090682536c8406c321 HG_TAG=a 
  precommit hook: HG_PARENT1=4c52fb2e402287dd5dc052090682536c8406c321 
  pretxncommit hook: HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_PARENT1=4c52fb2e402287dd5dc052090682536c8406c321 HG_PENDING=$HGTMP/test-hook.t/a 
  4:8ea2ef7ad3e8
  commit hook: HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_PARENT1=4c52fb2e402287dd5dc052090682536c8406c321 
  commit.b hook: HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_PARENT1=4c52fb2e402287dd5dc052090682536c8406c321 
  tag hook: HG_LOCAL=0 HG_NODE=4c52fb2e402287dd5dc052090682536c8406c321 HG_TAG=a 
  $ hg tag -l la
  pretag hook: HG_LOCAL=1 HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_TAG=la 
  tag hook: HG_LOCAL=1 HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_TAG=la 

# pretag hook can forbid tagging

  $ echo 'pretag.forbid = python ../printenv.py pretag.forbid 1' >> .hg/hgrc
  $ hg tag -d '4 0' fa
  pretag hook: HG_LOCAL=0 HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_TAG=fa 
  pretag.forbid hook: HG_LOCAL=0 HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_TAG=fa 
  abort: pretag.forbid hook exited with status 1
  $ hg tag -l fla
  pretag hook: HG_LOCAL=1 HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_TAG=fla 
  pretag.forbid hook: HG_LOCAL=1 HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_TAG=fla 
  abort: pretag.forbid hook exited with status 1

# pretxncommit hook can see changeset, can roll back txn, changeset
# no more there after

  $ echo 'pretxncommit.forbid0 = hg tip -q' >> .hg/hgrc
  $ echo 'pretxncommit.forbid1 = python ../printenv.py pretxncommit.forbid 1' >> .hg/hgrc
  $ echo z > z
  $ hg add z
  $ hg -q tip
  4:8ea2ef7ad3e8
  $ hg commit -m 'fail' -d '4 0'
  precommit hook: HG_PARENT1=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 
  pretxncommit hook: HG_NODE=fad284daf8c032148abaffcd745dafeceefceb61 HG_PARENT1=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_PENDING=$HGTMP/test-hook.t/a 
  5:fad284daf8c0
  5:fad284daf8c0
  pretxncommit.forbid hook: HG_NODE=fad284daf8c032148abaffcd745dafeceefceb61 HG_PARENT1=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_PENDING=$HGTMP/test-hook.t/a 
  transaction abort!
  rollback completed
  abort: pretxncommit.forbid1 hook exited with status 1
  $ hg -q tip
  4:8ea2ef7ad3e8

# precommit hook can prevent commit

  $ echo 'precommit.forbid = python ../printenv.py precommit.forbid 1' >> .hg/hgrc
  $ hg commit -m 'fail' -d '4 0'
  precommit hook: HG_PARENT1=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 
  precommit.forbid hook: HG_PARENT1=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 
  abort: precommit.forbid hook exited with status 1
  $ hg -q tip
  4:8ea2ef7ad3e8

# preupdate hook can prevent update

  $ echo 'preupdate = python ../printenv.py preupdate' >> .hg/hgrc
  $ hg update 1
  preupdate hook: HG_PARENT1=b702efe96888 
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved

# update hook

  $ echo 'update = python ../printenv.py update' >> .hg/hgrc
  $ hg update
  preupdate hook: HG_PARENT1=8ea2ef7ad3e8 
  update hook: HG_ERROR=0 HG_PARENT1=8ea2ef7ad3e8 
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

# prechangegroup hook can prevent incoming changes

  $ cd ../b
  $ hg -q tip
  3:4c52fb2e4022
  $ echo '[hooks]' > .hg/hgrc
  $ echo 'prechangegroup.forbid = python ../printenv.py prechangegroup.forbid 1' >> .hg/hgrc
  $ hg pull ../a
  prechangegroup.forbid hook: HG_SOURCE=pull HG_URL=file: 
  pulling from ../a
  searching for changes
  abort: prechangegroup.forbid hook exited with status 1

# pretxnchangegroup hook can see incoming changes, can roll back txn,
# incoming changes no longer there after

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'pretxnchangegroup.forbid0 = hg tip -q' >> .hg/hgrc
  $ echo 'pretxnchangegroup.forbid1 = python ../printenv.py pretxnchangegroup.forbid 1' >> .hg/hgrc
  $ hg pull ../a
  4:8ea2ef7ad3e8
  pretxnchangegroup.forbid hook: HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_PENDING=$HGTMP/test-hook.t/b HG_SOURCE=pull HG_URL=file: 
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  transaction abort!
  rollback completed
  abort: pretxnchangegroup.forbid1 hook exited with status 1
  $ hg -q tip
  3:4c52fb2e4022

# outgoing hooks can see env vars

  $ rm .hg/hgrc
  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing = python ../printenv.py preoutgoing' >> ../a/.hg/hgrc
  $ echo 'outgoing = python ../printenv.py outgoing' >> ../a/.hg/hgrc
  $ hg pull ../a
  preoutgoing hook: HG_SOURCE=pull 
  outgoing hook: HG_NODE=8ea2ef7ad3e8cac946c72f1e0c79d6aebc301198 HG_SOURCE=pull 
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg rollback
  rolling back to revision 3 (undo pull)

# preoutgoing hook can prevent outgoing changes

  $ echo 'preoutgoing.forbid = python ../printenv.py preoutgoing.forbid 1' >> ../a/.hg/hgrc
  $ hg pull ../a
  preoutgoing hook: HG_SOURCE=pull 
  preoutgoing.forbid hook: HG_SOURCE=pull 
  pulling from ../a
  searching for changes
  abort: preoutgoing.forbid hook exited with status 1

# outgoing hooks work for local clones

  $ cd ..
  $ echo '[hooks]' > a/.hg/hgrc
  $ echo 'preoutgoing = python ../printenv.py preoutgoing' >> a/.hg/hgrc
  $ echo 'outgoing = python ../printenv.py outgoing' >> a/.hg/hgrc
  $ hg clone a c
  preoutgoing hook: HG_SOURCE=clone 
  outgoing hook: HG_NODE=0000000000000000000000000000000000000000 HG_SOURCE=clone 
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf c

# preoutgoing hook can prevent outgoing changes for local clones

  $ echo 'preoutgoing.forbid = python ../printenv.py preoutgoing.forbid 1' >> a/.hg/hgrc
  $ hg clone a zzz
  preoutgoing hook: HG_SOURCE=clone 
  preoutgoing.forbid hook: HG_SOURCE=clone 
  abort: preoutgoing.forbid hook exited with status 1
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
  > class container:
  >     unreachable = 1
  > EOF

# test python hooks

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

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.fail = python:hooktests.failhook' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  hook args:
    hooktype preoutgoing
    source pull
  abort: preoutgoing.fail hook failed

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.uncallable = python:hooktests.uncallable' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.uncallable hook is invalid ("hooktests.uncallable" is not callable)

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.nohook = python:hooktests.nohook' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.nohook hook is invalid ("hooktests.nohook" is not defined)

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.nomodule = python:nomodule' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.nomodule hook is invalid ("nomodule" not in a module)

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.badmodule = python:nomodule.nowhere' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.badmodule hook is invalid (import of "nomodule" failed)

  $ echo '[hooks]' > ../a/.hg/hgrc
  $ echo 'preoutgoing.unreachable = python:hooktests.container.unreachable' >> ../a/.hg/hgrc
  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: preoutgoing.unreachable hook is invalid (import of "hooktests.container" failed)

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

# make sure --traceback works

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
  $ hg ci --debug -d '0 0' -m 'change foo' | sed -e 's/ at .*>/>/'
  foo
  calling hook commit.auto: <function autohook>
  Automatically installed hook
  committed changeset 1:52998019f6252a2b893452765fcb0a47351a5708

  $ hg showconfig hooks | sed -e 's/ at .*>/>/'
  hooks.commit.auto=<function autohook>

# test python hook configured with python:[file]:[hook] syntax

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

  $ cd ../../b

# make sure --traceback works on hook import failure

  $ cat > importfail.py <<EOF
  > import somebogusmodule
  > # dereference something in the module to force demandimport to load it
  > somebogusmodule.whatever
  > EOF

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'precommit.importfail = python:importfail.whatever' >> .hg/hgrc

  $ echo a >> a
  $ hg --traceback commit -d '0 0' -ma 2>&1 | egrep '^(exception|Traceback|ImportError)'
  exception from first failed import attempt:
  Traceback (most recent call last):
  ImportError: No module named somebogusmodule
  exception from second failed import attempt:
  Traceback (most recent call last):
  ImportError: No module named hgext_importfail
  Traceback (most recent call last):

# commit and update hooks should run after command completion (issue 1827)

  $ echo '[hooks]' > .hg/hgrc
  $ echo 'commit = hg id' >> .hg/hgrc
  $ echo 'update = hg id' >> .hg/hgrc
  $ echo bb > a
  $ hg ci -d '0 0' -ma
  8da618c33484 tip
  $ hg up 0
  29b62aeb769f
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ exit 0
