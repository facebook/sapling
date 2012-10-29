This test demonstrates how Hg works with remote Hg bookmarks compared with
remote branches via Hg-Git.  Ideally, they would behave identically.  In
practice, some differences are unavoidable, but we should try to minimize
them.

This test should not bother testing the behavior of bookmark creation,
deletion, activation, deactivation, etc.  These behaviors, while important to
the end user, don't vary at all when Hg-Git is in use.  Only the synchonization
of bookmarks should be considered "under test", and mutation of bookmarks
locally is only to provide a test fixture.

bail if the user does not have git command-line client
  $ "$TESTDIR/hghave" git || exit 80

Bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

Skip if Mercurial < 2.1; workflow was different before that
  $ python -c 'from mercurial import util ; assert \
  >  util.version() != "unknown" and util.version() >= "2.1"' || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH

  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE

  $ gitcount=10
  $ gitcommit()
  > {
  >     GIT_AUTHOR_DATE="2007-01-01 00:00:$gitcount +0000"
  >     GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  >     git commit "$@" >/dev/null 2>/dev/null || echo "git commit error"
  >     gitcount=`expr $gitcount + 1`
  > }
  $ hgcount=10
  $ hgcommit()
  > {
  >     HGDATE="2007-01-01 00:00:$hgcount +0000"
  >     hg commit -u "test <test@example.org>" -d "$HGDATE" "$@" >/dev/null 2>/dev/null || echo "hg commit error"
  >     hgcount=`expr $hgcount + 1`
  > }
  $ gitstate()
  > {
  >     git log --format="  %h \"%s\" refs:%d" $@ | sed 's/HEAD, //'
  > }
  $ hgstate()
  > {
  >     hg log --template "  {rev} {node|short} \"{desc}\" bookmarks: [{bookmarks}]\n" $@
  > }
  $ hggitstate()
  > {
  >     hg log --template "  {rev} {node|short} {gitnode|short} \"{desc}\" bookmarks: [{bookmarks}]\n" $@
  > }

Initialize remote hg and git repos with equivalent initial contents
  $ hg init hgremoterepo
  $ cd hgremoterepo
  $ hg bookmark master
  $ for f in alpha beta gamma delta; do
  >     echo $f > $f; hg add $f; hgcommit -m "add $f"
  > done
  $ hg bookmark -r 1 b1
  $ hgstate
    3 fc2664cac217 "add delta" bookmarks: [master]
    2 d85ced7ae9d6 "add gamma" bookmarks: []
    1 7bcd915dc873 "add beta" bookmarks: [b1]
    0 3442585be8a6 "add alpha" bookmarks: []
  $ cd ..
  $ git init -q gitremoterepo
  $ cd gitremoterepo
  $ for f in alpha beta gamma delta; do
  >     echo $f > $f; git add $f; gitcommit -m "add $f"
  > done
  $ git branch b1 9497a4e
  $ gitstate
    55b133e "add delta" refs: (master)
    d338971 "add gamma" refs:
    9497a4e "add beta" refs: (b1)
    7eeab2e "add alpha" refs:
  $ cd ..

Cloning transfers all bookmarks from remote to local
  $ hg clone -q hgremoterepo purehglocalrepo
  $ cd purehglocalrepo
  $ hgstate
    3 fc2664cac217 "add delta" bookmarks: [master]
    2 d85ced7ae9d6 "add gamma" bookmarks: []
    1 7bcd915dc873 "add beta" bookmarks: [b1]
    0 3442585be8a6 "add alpha" bookmarks: []
  $ cd ..
  $ hg clone -q gitremoterepo hggitlocalrepo
  $ cd hggitlocalrepo
  $ hggitstate
    3 fc2664cac217 55b133e1d558 "add delta" bookmarks: [master]
    2 d85ced7ae9d6 d338971a96e2 "add gamma" bookmarks: []
    1 7bcd915dc873 9497a4ee62e1 "add beta" bookmarks: [b1]
    0 3442585be8a6 7eeab2ea75ec "add alpha" bookmarks: []
  $ cd ..

No changes
  $ cd purehglocalrepo
  $ hg outgoing
  comparing with $TESTTMP/hgremoterepo
  searching for changes
  no changes found
  [1]
  $ hg outgoing -B
  comparing with $TESTTMP/hgremoterepo
  searching for changed bookmarks
  no changed bookmarks found
  [1]
  $ hg push
  pushing to $TESTTMP/hgremoterepo
  searching for changes
  no changes found
  [1]
  $ cd ..
  $ cd hggitlocalrepo
  $ hg outgoing
  comparing with $TESTTMP/gitremoterepo
  searching for changes
  no changes found
  [1]
  $ hg outgoing -B
  comparing with $TESTTMP/gitremoterepo
  searching for changed bookmarks
  no changed bookmarks found
  [1]
  $ hg push
  pushing to $TESTTMP/gitremoterepo
  searching for changes
  no changes found
  [1]
  $ cd ..

Changed bookmarks, but not revs
  $ cd purehglocalrepo
  $ hg bookmark -fr 2 b1
  $ hg bookmark -r 0 b2
  $ hgstate
    3 fc2664cac217 "add delta" bookmarks: [master]
    2 d85ced7ae9d6 "add gamma" bookmarks: [b1]
    1 7bcd915dc873 "add beta" bookmarks: []
    0 3442585be8a6 "add alpha" bookmarks: [b2]
  $ hg outgoing
  comparing with $TESTTMP/hgremoterepo
  searching for changes
  no changes found
  [1]
As of 2.3, Mercurial's outgoing -B doesn't actually show changed bookmarks
It only shows "new" bookmarks.  Thus, b1 doesn't show up.
  $ hg outgoing -B
  comparing with $TESTTMP/hgremoterepo
  searching for changed bookmarks
     b2                        3442585be8a6
  $ cd ..
  $ cd hggitlocalrepo
  $ hg bookmark -fr 2 b1
  $ hg bookmark -r 0 b2
  $ hgstate
    3 fc2664cac217 "add delta" bookmarks: [master]
    2 d85ced7ae9d6 "add gamma" bookmarks: [b1]
    1 7bcd915dc873 "add beta" bookmarks: []
    0 3442585be8a6 "add alpha" bookmarks: [b2]
  $ hg outgoing
  comparing with $TESTTMP/gitremoterepo
  searching for changes
  no changes found
  [1]
As of 2.3, Mercurial's outgoing -B doesn't actually show changed bookmarks
It only shows "new" bookmarks.  Thus, b1 doesn't show up.
  $ hg outgoing -B
  comparing with $TESTTMP/gitremoterepo
  searching for changed bookmarks
     b2                        3442585be8a6
  $ cd ..
