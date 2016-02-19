Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

Test that hg pull creates obsolescence markers for landed diffs
  $ $PYTHON -c 'import remotenames' || exit 80
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > username = nobody <no.reply@fb.com>
  > ssh = python "$RUNTESTDIR/dummyssh"
  > [experimental]
  > evolution= createmarkers
  > rebaseskipobsolete=True
  > [extensions]
  > inhibit=
  > directaccess=
  > evolve=
  > strip=
  > rebase=
  > remotenames =
  > bundle2hooks =
  > pushrebase =
  > pullcreatemarkers= $TESTDIR/../pullcreatemarkers.py
  > [remotenames]
  > allownonfastforward=True
  > EOF
  $ hg init server
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    [ -z "$2" ] || echo "Differential Revision: https://phabricator.fb.com/D$2" >> msg
  >    hg ci -l msg
  > }

Set up server repository

  $ cd server
  $ mkcommit initial
  $ mkcommit secondcommit
  $ hg book master
  $ cd ..

Set up clients repository

  $ hg clone ssh://user@dummy/server client -q
  $ hg clone ssh://user@dummy/server otherclient -q

The first client works on a diff while the second client lands one of her diff

  $ cd otherclient
  $ mkcommit b
  $ hg push --to master
  pushing rev 2e73b79a63d8 to destination ssh://user@dummy/server bookmark master
  searching for changes
  remote: pushing 1 commit:
  remote:     2e73b79a63d8  add b
  updating bookmark master
  $ cd ../client
  $ mkcommit c 123 # 123 is the phabricator rev number (see function above)
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  @  2 "add c
  |
  |  Differential Revision: https://phabricator.fb.com/D123"
  o  1 "add secondcommit" default/master
  |
  o  0 "add initial"
  
  $ hg push --to master
  pushing rev 1a07332e9fa1 to destination ssh://user@dummy/server bookmark master
  searching for changes
  remote: pushing 1 commit:
  remote:     1a07332e9fa1  add c
  remote: 2 new commits from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  updating bookmark master

Here we strip commit 4 to simulate what happens with landcastle, the push
don't directly go to the server

  $ hg strip 4
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/d446b1b2be43-5832344b-backup.hg (glob)

We update to commit 1 to avoid keeping 2 visible with inhibit

  $ hg update 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Here pull should mark 2 as obsolete since it landed as 4 on the remote
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  o  3 "add b"
  |
  | o  2 "add c
  |/
  |    Differential Revision: https://phabricator.fb.com/D123"
  @  1 "add secondcommit"
  |
  o  0 "add initial"
  
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  o  4 "add c
  |
  |  Differential Revision: https://phabricator.fb.com/D123" default/master
  o  3 "add b"
  |
  @  1 "add secondcommit"
  |
  o  0 "add initial"
  
Rebasing a stack containing landed changesets should only rebase the non-landed
changesets

  $ hg up --hidden 2 # --hidden because directaccess works only with hashes
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit k
  $ hg rebase -r ".^ + ." -d 4
  note: not rebasing 2:1a07332e9fa1 "add c", already in destination as 4:d446b1b2be43 "add c"
  rebasing 5:13e6318883c9 "add k" (tip)

