#debugruntest-compatible
#require icasefs
#require no-windows

  $ configure modernclient

Status is clean when file changes case
  $ newclientrepo
  $ touch file
  $ hg commit -Aqm foo
  $ mv file FILE
  $ hg st

Status keeps removed file and untracked file separate
  $ newclientrepo
  $ touch file
  $ hg commit -Aqm foo
  $ hg rm file
  $ touch FILE
  $ hg st
  R file
  ? FILE

Status is clean when directory changes case
  $ newclientrepo
  $ mkdir dir
  $ echo foo > dir/file
  $ hg commit -Aqm foo
  $ rm -rf dir
  $ mkdir DIR
  $ echo foo > DIR/file
  $ hg st

When new file's dir on disk disagrees w/ case in treestate, use treestate's case:
  $ newclientrepo
  $ mkdir dir
  $ touch dir/file
  $ hg commit -Aqm foo
  $ touch dir/file2
  $ mv dir DIR
  $ hg add -q DIR/file2
Show as dir/file2, not DIR/file2 (this avoids treestate divergence)
  $ hg st
  A dir/file2

Test behavior when checking out across directory case change:
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/dir/FILE = foo (renamed from dir/file)
  > |
  > A  # A/dir/file = foo
  >    # drawdag.defaultfiles=false
  > EOS
  $ hg go -q $B
  $ find .
  dir
  dir/FILE
  $ hg go -q $A
  $ find .
  dir
  dir/file
  $ hg mv -q dir temp
  $ hg mv -q temp DIR
  $ hg commit -qm uppercase
  $ find .
  DIR
  DIR/file
  $ hg go -q '.^'
  $ find .
  dir
  dir/file
  $ hg go -q 'desc(uppercase)'
Checkout across the same change, but this time there is an untracked
file in the directory. This time the directory is not made lowercase,
since it is not deleted due to the presence of the untracked file.
  $ touch dir/untracked
  $ hg go -q '.^'
  $ find .
  DIR
  DIR/file
  DIR/untracked
This mismatch occurs because the directory is "DIR" in the treestate when "status" is run
at the beginning of the above "go" operation, so fsmonitor records in treestate as
"DIR/untracked". We don't have a process to update "DIR/untracked" to "dir/untracked" to
match the tracked file "dir/file".
  $ hg st
  ? dir/untracked (no-fsmonitor !)
  ? DIR/untracked (fsmonitor !)


Sparse profile rules are case sensitive:
  $ newclientrepo
  $ enable sparse
  $ mkdir included excluded
  $ touch included/file excluded/file
  $ hg commit -Aqm foo
  $ hg sparse include included
  $ find .
  included
  included/file
  $ hg sparse reset
  $ hg sparse include INCLUDED
  $ find .


Gitignore filters files case-insensitively:
  $ newclientrepo
  $ touch .gitignore
  $ mkdir included excluded
  $ touch included/file excluded/file
  $ hg commit -Aqm foo .gitignore
  $ hg st -u
  ? excluded/file
  ? included/file
  $ echo included > .gitignore
  $ hg st -u
  ? excluded/file
  $ echo INCLUDED > .gitignore
  $ hg st -u
  ? excluded/file
