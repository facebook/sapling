  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export BLOCK_MERGES=1
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

Setup helpers
  $ log() {
  >   hg log -G -T "{desc} [{phase};rev={rev};{node|short}] {remotenames}" "$@"
  > }
setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Clone the repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

Try to push merge commit
  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg up -q 0
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ hg merge -q -r 3 && hg ci -m "merge 1 and 2"
  $ log -r ":"
  @    merge 1 and 2 [draft;rev=5;3e1c4ca1f9be]
  |\
  | o  2 [draft;rev=4;c9b2673d3218]
  | |
  o |  1 [draft;rev=3;a0c9c5791058]
  |/
  | o  C [public;rev=2;26805aba1e60] default/master_bookmark
  | |
  | o  B [public;rev=1;112478962961]
  |/
  o  A [public;rev=0;426bada5c675]
  
  $ hgmn push -r . --to master_bookmark
  pushing rev 3e1c4ca1f9be to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Pushrebase blocked because it contains a merge commit.
  remote:     If you need this for a specific use case please contact
  remote:     the Source Control team at https://fburl.com/27qnuyl2
  remote:   Root cause:
  remote:     "Pushrebase blocked because it contains a merge commit.\nIf you need this for a specific use case please contact\nthe Source Control team at https://fburl.com/27qnuyl2"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
