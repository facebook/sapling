  $ setconfig extensions.treemanifest=!

Setup


Test that hg pull creates obsolescence markers for landed diffs
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > username = nobody <no.reply@fb.com>
  > ssh = python "$RUNTESTDIR/dummyssh"
  > [experimental]
  > evolution= createmarkers
  > rebaseskipobsolete=True
  > [extensions]
  > amend=
  > pullcreatemarkers=
  > pushrebase=
  > rebase=
  > remotenames =
  > [remotenames]
  > allownonfastforward=True
  > EOF
  $ hg init server
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    url="https://phabricator.fb.com"
  >    if [ -n "$3" ]; then
  >      url="$3"
  >    fi
  >    [ -z "$2" ] || echo "Differential Revision: $url/D$2" >> msg
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

The first client works on several diffs while the second client lands one of her diff

  $ cd otherclient
  $ mkcommit b
  $ hg push --to master
  pushing rev 2e73b79a63d8 to destination ssh://user@dummy/server bookmark master
  searching for changes
  remote: pushing 1 changeset:
  remote:     2e73b79a63d8  add b
  updating bookmark master
  $ cd ../client
  $ mkcommit c 123 # 123 is the phabricator rev number (see function above)
  $ mkcommit d 124 "https://phabricator.intern.facebook.com"
  $ mkcommit e 131
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  @  4 "add e
  |
  |  Differential Revision: https://phabricator.fb.com/D131"
  o  3 "add d
  |
  |  Differential Revision: https://phabricator.intern.facebook.com/D124"
  o  2 "add c
  |
  |  Differential Revision: https://phabricator.fb.com/D123"
  o  1 "add secondcommit" default/master
  |
  o  0 "add initial"
  
  $ hg push --to master
  pushing rev d5895ab36037 to destination ssh://user@dummy/server bookmark master
  searching for changes
  remote: pushing 3 changesets:
  remote:     1a07332e9fa1  add c
  remote:     ee96b78ae17d  add d
  remote:     d5895ab36037  add e
  remote: 4 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 1 changes to 4 files (+1 heads)
  3 new obsolescence markers
  updating bookmark master
  obsoleted 3 changesets
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Here we strip commits 6, 7, 8 to simulate what happens with landcastle, the
push doesn't directly go to the server

  $ hg debugstrip 6
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/d446b1b2be43-516f7371-backup.hg (glob)

We update to commit 1 to avoid keeping 2, 3, and 4 visible with inhibit

  $ hg update 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Here pull should mark 2, 3, and 4 as obsolete since they landed as 6, 7, 8 on
the remote
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  o  5 "add b"
  |
  | o  4 "add e
  | |
  | |  Differential Revision: https://phabricator.fb.com/D131"
  | o  3 "add d
  | |
  | |  Differential Revision: https://phabricator.intern.facebook.com/D124"
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
  added 3 changesets with 0 changes to 3 files
  new changesets d446b1b2be43:461a5b25b3dc
  obsoleted 3 changesets
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  o  8 "add e
  |
  |  Differential Revision: https://phabricator.fb.com/D131" default/master
  o  7 "add d
  |
  |  Differential Revision: https://phabricator.intern.facebook.com/D124"
  o  6 "add c
  |
  |  Differential Revision: https://phabricator.fb.com/D123"
  o  5 "add b"
  |
  @  1 "add secondcommit"
  |
  o  0 "add initial"
  
Rebasing a stack containing landed changesets should only rebase the non-landed
changesets

  $ hg up --hidden 4 # --hidden because directaccess works only with hashes
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit k 202
  $ hg rebase -d default/master
  note: not rebasing 2:1a07332e9fa1 "add c", already in destination as 6:d446b1b2be43 "add c"
  note: not rebasing 3:ee96b78ae17d "add d", already in destination as 7:1f539cc6f364 "add d"
  note: not rebasing 4:d5895ab36037 "add e", already in destination as 8:461a5b25b3dc "add e" (default/master master)
  rebasing 9:7dcd118e395a "add k" (tip)

  $ echo more >> k
  $ hg amend
  $ hg unhide 10

  $ cd ../server
  $ mkcommit k 202
  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)
  new changesets a4884e89c0d5
  obsoleted 1 changesets

(Note: pullcreatemarkers created two markers, however only one of them was
counted in the message as the first commit had previously been obsoleted
and revived)
