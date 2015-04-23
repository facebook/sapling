Verify that pending changesets are seen by pretxn* hooks but not by other
processes that access the destination repo while the hooks are running.

The hooks (python and external) both reject changesets after some think time,
during which another process runs pull.  Each hook creates a file ('notify') to
indicate to the controlling process that it is running; the process removes the
file to indicate the hook can terminate.

init env vars

  $ d=`pwd`
  $ maxwait=20

utility to run the test - start a push in the background and run pull

  $ dotest() {
  >     rm -f notify
  >     printf 'push '; hg -R child-push tip --template '{node}\n'
  >     hg -R child-push -q push > push.out 2>&1 &
  > 
  >     # wait for hook to create the notify file
  >     i=$maxwait
  >     while [ ! -f notify -a $i != 0 ]; do
  >         sleep 1
  >         i=`expr $i - 1`
  >     done
  > 
  >     # run pull
  >     hg -R child-pull -q pull
  >     rc=$?
  > 
  >     # tell hook to finish; notify should exist.
  >     rm notify
  >     wait
  > 
  >     cat push.out
  >     printf 'pull '; hg -R child-pull tip --template '{node}\n'
  >     return $rc
  > }

python hook

  $ cat <<EOF > reject.py
  > import os, time
  > from mercurial import ui, localrepo
  > def rejecthook(ui, repo, hooktype, node, **opts):
  >     ui.write('hook %s\\n' % repo['tip'].hex())
  >     # create the notify file so caller knows we're running
  >     fpath = os.path.join('$d', 'notify')
  >     f = open(fpath, 'w')
  >     f.close()
  >     # wait for ack - caller should delete the notify file
  >     i = $maxwait
  >     while os.path.exists(fpath) and i > 0:
  >         time.sleep(1)
  >         i -= 1
  >     return True # reject the changesets
  > EOF

external hook

  $ cat <<EOF > reject.sh
  > printf 'hook '; hg tip --template '{node}\\n'
  > # create the notify file so caller knows we're running
  > fpath=$d/notify
  > touch \$fpath
  > # wait for ack - caller should delete the notify file
  > i=$maxwait
  > while [ -f \$fpath -a \$i != 0 ]; do
  >     sleep 1
  >     i=\`expr \$i - 1\`
  > done
  > exit 1 # reject the changesets
  > EOF

create repos

  $ hg init parent
  $ hg clone -q parent child-push
  $ hg clone -q parent child-pull
  $ echo a > child-push/a
  $ hg -R child-push add child-push/a
  $ hg -R child-push commit -m a -d '1000000 0'

test python hook

  $ cat <<EOF > parent/.hg/hgrc
  > [extensions]
  > reject = $d/reject.py
  > [hooks]
  > pretxnchangegroup = python:reject.rejecthook
  > EOF

  $ dotest
  push 29b62aeb769fdf78d8d9c5f28b017f76d7ef824b
  hook 29b62aeb769fdf78d8d9c5f28b017f76d7ef824b
  transaction abort!
  rollback completed
  abort: pretxnchangegroup hook failed
  pull 0000000000000000000000000000000000000000

test external hook

  $ cat <<EOF > parent/.hg/hgrc
  > [hooks]
  > pretxnchangegroup = sh $d/reject.sh
  > EOF

  $ dotest
  push 29b62aeb769fdf78d8d9c5f28b017f76d7ef824b
  hook 29b62aeb769fdf78d8d9c5f28b017f76d7ef824b
  transaction abort!
  rollback completed
  abort: pretxnchangegroup hook exited with status 1
  pull 0000000000000000000000000000000000000000

Test that pending on transaction without changegroup see the normal changegroup(
(issue4609)

  $ cat <<EOF > parent/.hg/hgrc
  > [hooks]
  > pretxnchangegroup=
  > pretxnclose = hg tip -T "tip: {node|short}\n"
  > [phases]
  > publishing=False
  > EOF

setup

  $ cd parent
  $ echo a > a
  $ hg add a
  $ hg commit -m a
  tip: cb9a9f314b8b

actual test

  $ hg phase --public .
  tip: cb9a9f314b8b
