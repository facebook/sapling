  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq =
  > [mq]
  > git = keep
  > [diff]
  > nodates = 1
  > EOF

init:

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo a >> a
  $ hg qnew -f p1
  $ echo b >> a
  $ hg qnew -f p2
  $ echo c >> a
  $ hg qnew -f p3

Fold in the middle of the queue:
(this tests also that editor is not invoked if '--edit' is not
specified)

  $ hg qpop p1
  popping p3
  popping p2
  now at: p1

  $ hg qdiff
  diff -r 07f494440405 a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +a

  $ HGEDITOR=cat hg qfold p2
  $ grep git .hg/patches/p1 && echo 'git patch found!'
  [1]

  $ hg qser
  p1
  p3

  $ hg qdiff
  diff -r 07f494440405 a
  --- a/a
  +++ b/a
  @@ -1,1 +1,3 @@
   a
  +a
  +b

Fold with local changes:

  $ echo d >> a
  $ hg qfold p3
  abort: local changes found, qrefresh first
  [255]

  $ hg diff -c .
  diff -r 07f494440405 -r ???????????? a (glob)
  --- a/a
  +++ b/a
  @@ -1,1 +1,3 @@
   a
  +a
  +b

  $ hg revert -a --no-backup
  reverting a

Fold git patch into a regular patch, expect git patch:

  $ echo a >> a
  $ hg qnew -f regular
  $ hg cp a aa
  $ hg qnew --git -f git

  $ hg qpop
  popping git
  now at: regular

  $ hg qfold git

  $ cat .hg/patches/regular
  # HG changeset patch
  # Parent  ???????????????????????????????????????? (glob)
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,4 @@
   a
   a
   b
  +a
  diff --git a/a b/aa
  copy from a
  copy to aa
  --- a/a
  +++ b/aa
  @@ -1,3 +1,4 @@
   a
   a
   b
  +a

  $ hg qpop
  popping regular
  now at: p1

  $ hg qdel regular

Fold regular patch into a git patch, expect git patch:

  $ hg cp a aa
  $ hg qnew --git -f git
  $ echo b >> aa
  $ hg qnew -f regular

  $ hg qpop
  popping regular
  now at: git

  $ hg qfold regular

  $ cat .hg/patches/git
  # HG changeset patch
  # Parent  ???????????????????????????????????????? (glob)
  
  diff --git a/a b/aa
  copy from a
  copy to aa
  --- a/a
  +++ b/aa
  @@ -1,3 +1,4 @@
   a
   a
   b
  +b

Test saving last-message.txt:

  $ hg qrefresh -m "original message"

  $ cat > $TESTTMP/commitfailure.py <<EOF
  > from mercurial import error
  > def reposetup(ui, repo):
  >     class commitfailure(repo.__class__):
  >         def commit(self, *args, **kwargs):
  >             raise error.Abort('emulating unexpected abort')
  >     repo.__class__ = commitfailure
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > # this failure occurs before editor invocation
  > commitfailure = $TESTTMP/commitfailure.py
  > EOF

  $ cat > $TESTTMP/editor.sh << EOF
  > echo "==== before editing"
  > cat \$1
  > echo "===="
  > (echo; echo "test saving last-message.txt") >> \$1
  > EOF

  $ hg qapplied
  p1
  git
  $ hg tip --template "{files}\n"
  aa

(test that editor is not invoked before transaction starting,
and that combination of '--edit' and '--message' doesn't abort execution)

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg qfold -e -m MESSAGE p3
  qrefresh interrupted while patch was popped! (revert --all, qpush to recover)
  abort: emulating unexpected abort
  [255]
  $ test -f .hg/last-message.txt
  [1]

(reset applied patches and directory status)

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > # this failure occurs after editor invocation
  > commitfailure = !
  > EOF

  $ hg qapplied
  p1
  $ hg status -A aa
  ? aa
  $ rm aa
  $ hg status -m
  M a
  $ hg revert --no-backup -q a
  $ hg qpush -q git
  now at: git

(test that editor is invoked and commit message is saved into
"last-message.txt")

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > # this failure occurs after editor invocation
  > pretxncommit.unexpectedabort = false
  > EOF

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg qfold -e p3
  ==== before editing
  original message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to use default message.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added aa
  HG: changed a
  ====
  note: commit message saved in .hg/last-message.txt
  transaction abort!
  rollback completed
  qrefresh interrupted while patch was popped! (revert --all, qpush to recover)
  abort: pretxncommit.unexpectedabort hook exited with status 1
  [255]
  $ cat .hg/last-message.txt
  original message
  
  
  
  test saving last-message.txt

(confirm whether files listed up in the commit message editing are correct)

  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > pretxncommit.unexpectedabort =
  > EOF
  $ hg status -u | while read f; do rm ${f}; done
  $ hg revert --no-backup -q --all
  $ hg qpush -q git
  now at: git
  $ hg qpush -q --move p3
  now at: p3

  $ hg status --rev "git^1" --rev . -arm
  M a
  A aa

  $ cd ..

