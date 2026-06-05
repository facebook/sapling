#require icasefs
#require no-windows

Status is clean when file changes case
  $ newclientrepo
  $ touch file
  $ sl commit -Aqm foo
  $ mv file FILE
  $ sl st

Status keeps removed file and untracked file separate
  $ newclientrepo
  $ touch file
  $ sl commit -Aqm foo
  $ sl rm file
  $ touch FILE
TODO(sggutier): EdenFS behaves differently here
  $ sl st
  R file
  ? FILE (no-eden !)

Status is clean when directory changes case
  $ newclientrepo
  $ mkdir dir
  $ echo foo > dir/file
  $ sl commit -Aqm foo
  $ rm -rf dir
  $ mkdir DIR
  $ echo foo > DIR/file
  $ sl st

When new file's dir on disk disagrees w/ case in treestate, use treestate's case:
  $ newclientrepo
  $ mkdir dir
  $ touch dir/file
  $ sl commit -Aqm foo
  $ touch dir/file2
  $ mv dir DIR
  $ sl add -q DIR/file2
Show as dir/file2, not DIR/file2 (this avoids treestate divergence)
  $ sl st
  A dir/file2

Test behavior when checking out across directory case change:
  $ newclientrepo
  $ drawdag <<EOS
  > B  # B/dir/FILE = foo (renamed from dir/file)
  > |
  > A  # A/dir/file = foo
  >    # drawdag.defaultfiles=false
  > EOS
  $ sl go -q $B
  $ find .
  ./dir
  ./dir/FILE
  $ sl go -q $A
  $ find .
  ./dir
  ./dir/file
  $ sl mv -q dir temp
  $ sl mv -q temp DIR
  $ sl commit -qm uppercase
  $ find .
  ./DIR
  ./DIR/file
#if no-eden
TODO(sggutier): EdenFS behaves differently here too, the goto fails
  $ sl go -q '.^'
  $ find .
  ./dir
  ./dir/file
  $ sl go -q 'desc(uppercase)'
Checkout across the same change, but this time there is an untracked
file in the directory. This time the directory is not made lowercase,
since it is not deleted due to the presence of the untracked file.
  $ touch dir/untracked
  $ sl go -q '.^'
  $ find .
  ./DIR
  ./DIR/file
  ./DIR/untracked
This mismatch occurs because the directory is "DIR" in the treestate when "status" is run
at the beginning of the above "go" operation, so fsmonitor records in treestate as
"DIR/untracked". We don't have a process to update "DIR/untracked" to "dir/untracked" to
match the tracked file "dir/file".
  $ sl st
  ? dir/untracked (no-fsmonitor !)
  ? DIR/untracked (fsmonitor !)
#endif


#if no-eden
Sparse profile rules are case sensitive:
  $ newclientrepo
  $ enable sparse
  $ mkdir included excluded
  $ touch included/file excluded/file
  $ sl commit -Aqm foo
  $ sl sparse include included
  $ find .
  ./included
  ./included/file
  $ sl sparse reset
  $ sl sparse include INCLUDED
  $ find .
#endif


Gitignore filters files case-insensitively:
  $ newclientrepo
  $ touch .gitignore
  $ mkdir included excluded
  $ touch included/file excluded/file
  $ sl commit -Aqm foo .gitignore
  $ sl st -u
  ? excluded/file
  ? included/file
  $ echo included > .gitignore
  $ sl st -u
  ? excluded/file
  $ echo INCLUDED > .gitignore
  $ sl st -u
  ? excluded/file
