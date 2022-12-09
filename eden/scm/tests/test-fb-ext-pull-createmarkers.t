#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest

Setup

  $ configure mutation-norecord dummyssh
  $ enable amend pullcreatemarkers pushrebase rebase remotenames
  $ setconfig ui.username="nobody <no.reply@fb.com>" experimental.rebaseskipobsolete=true
  $ setconfig remotenames.allownonfastforward=true

Test that hg pull creates obsolescence markers for landed diffs
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
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     2e73b79a63d8  add b
  $ cd ../client
  $ mkcommit c 123 # 123 is the phabricator rev number (see function above)
  $ mkcommit d 124 "https://phabricator.intern.facebook.com"
  $ mkcommit e 131
  $ hg log -G -T '"{desc}" {remotebookmarks}'
  @  "add e
  │
  │  Differential Revision: https://phabricator.fb.com/D131"
  o  "add d
  │
  │  Differential Revision: https://phabricator.intern.facebook.com/D124"
  o  "add c
  │
  │  Differential Revision: https://phabricator.fb.com/D123"
  o  "add secondcommit" default/master
  │
  o  "add initial"
  
  $ hg push --to master
  pushing rev d5895ab36037 to destination ssh://user@dummy/server bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 3 changesets:
  remote:     1a07332e9fa1  add c
  remote:     ee96b78ae17d  add d
  remote:     d5895ab36037  add e
  remote: 4 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Here we strip commits 6, 7, 8 to simulate what happens with landcastle, the
push doesn't directly go to the server

  $ hg debugstrip d446b1b2be434509eb0ed51c1da3056d1bc21d12
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved

We update to commit 1 to avoid keeping 2, 3, and 4 visible with inhibit

  $ hg goto 11b76ecbf1d49ab485207f46d8c45ee8c96b1bfb
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Here pull should mark 2, 3, and 4 as obsolete since they landed as 6, 7, 8 on
the remote
  $ hg log -G -T '"{desc}" {remotebookmarks}'
  @  "add secondcommit"
  │
  o  "add initial"
  
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg log -G -T '"{desc}" {remotebookmarks}'
  o  "add e
  │
  │  Differential Revision: https://phabricator.fb.com/D131" default/master
  o  "add d
  │
  │  Differential Revision: https://phabricator.intern.facebook.com/D124"
  o  "add c
  │
  │  Differential Revision: https://phabricator.fb.com/D123"
  o  "add b"
  │
  @  "add secondcommit"
  │
  o  "add initial"
  
Rebasing a stack containing landed changesets should only rebase the non-landed
changesets

  $ hg up --hidden d5895ab3603770985bf7ab04bf25c0da2d7e08ab # --hidden because directaccess works only with hashes
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit k 202
  $ hg rebase -d default/master
  note: not rebasing 1a07332e9fa1 "add c", already in destination as d446b1b2be43 "add c"
  note: not rebasing ee96b78ae17d "add d", already in destination as 1f539cc6f364 "add d"
  note: not rebasing d5895ab36037 "add e", already in destination as 461a5b25b3dc "add e" (default/master master)
  rebasing 7dcd118e395a "add k"

  $ echo more >> k
  $ hg amend
  $ hg unhide c34ae580ee12e8c648afba30a093690a6e018dac

  $ cd ../server
  $ mkcommit k 202
  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes

(Note: pullcreatemarkers created two markers, however only one of them was
counted in the message as the first commit had previously been obsoleted
and revived)
