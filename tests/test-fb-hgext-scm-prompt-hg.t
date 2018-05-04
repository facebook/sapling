To run this test against other shells, use the shell argument, eg:
run-tests.py --shell=zsh test-scm-prompt*

Initialize scm prompt
  $ . $TESTDIR/../contrib/scm-prompt.sh

  $ cmd() {
  >   "$@"
  >   _scm_prompt "(%s)\n"
  > }

Throw some ridiculous functions at it
  $ grep() {
  >   grep -V
  > }
  $ cat() {
  >   true
  > }

Outside of a repo, should have no output
  $ _scm_prompt

Test basic repo behaviors
  $ hg init repo
  $ cmd cd repo
  (empty)
  $ echo a > a
  $ cmd hg add a
  (0000000)
  $ cmd hg commit -m 'c1'
  (5cad84d)
  $ cmd hg book active
  (active)
  $ cmd hg book -i
  (5cad84d)
  $ echo b > b
  $ cmd hg add b
  (5cad84d)
  $ cmd hg commit -m 'c2'
  (775bfdd)
  $ cmd hg up -q active
  (active)
  $ echo bb > b
  $ hg add b
  $ cmd hg commit -m 'c3'
  (active)
  $ hg log -T '{node|short} {desc}\n' -G
  @  4b6cc7d5194b c3
  |
  | o  775bfdddc842 c2
  |/
  o  5cad84d1726f c1
  

Test rebase
  $ cmd hg rebase -d 775bfdd --config "extensions.rebase="
  rebasing 2:4b6cc7d5194b "c3" (active tip)
  merging b
  warning: conflicts while merging b! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  (775bfdd|REBASE)
  $ cmd hg book rebase
  (rebase|REBASE)
  $ cmd hg rebase --abort --config "extensions.rebase="
  rebase aborted
  (active)
  $ cmd hg book -i
  (4b6cc7d)

Test histedit
  $ command cat > commands <<EOF
  > edit 4b6cc7d5194b
  > EOF
  $ cmd hg histedit --config "extensions.histedit=" --commands commands
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  adding b
  Editing (4b6cc7d5194b), you may commit or record as needed now.
  (hg histedit --continue to resume)
  (5cad84d|HISTEDIT)
  $ cmd hg histedit --config "extensions.histedit=" --abort
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (4b6cc7d)

Test graft
  $ cmd hg graft 775bfdddc842
  grafting 1:775bfdddc842 "c2" (rebase)
  merging b
  warning: conflicts while merging b! (edit, then use 'hg resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'hg resolve' and 'hg graft --continue')
  (4b6cc7d|GRAFT)
  $ cmd hg revert -r 775bfdddc842 b
  (4b6cc7d|GRAFT)
  $ cmd hg resolve --mark b
  (no more unresolved files)
  continue: hg graft --continue
  (4b6cc7d|GRAFT)
  $ cmd hg graft --continue
  grafting 1:775bfdddc842 "c2" (rebase)
  (42eaf5c)

Test bisect
  $ cmd hg bisect -b .
  (42eaf5c|BISECT)
  $ cmd hg bisect -g ".^^"
  Testing changeset 2:4b6cc7d5194b (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (4b6cc7d|BISECT)
  $ cmd hg bisect -r
  (4b6cc7d)

Test unshelve
  $ echo b >> b
  $ cmd hg shelve --config "extensions.shelve="
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (4b6cc7d)
  $ cmd hg up -q ".^"
  (5cad84d)
  $ cmd hg unshelve --config "extensions.shelve="
  unshelving change 'default'
  rebasing shelved changes
  rebasing 4:29190234e4ef "changes to: c3" (tip)
  other [source] changed b which local [dest] deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  unresolved conflicts (see 'hg resolve', then 'hg unshelve --continue')
  (5cad84d|UNSHELVE)
  $ cmd hg unshelve --config "extensions.shelve=" --abort
  rebase aborted
  unshelve of 'default' aborted
  (5cad84d)

Test merge
  $ cmd hg up -q 4b6cc7d5194b
  (4b6cc7d)
  $ cmd hg merge 775bfdddc842
  merging b
  warning: conflicts while merging b! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  (4b6cc7d|MERGE)
  $ cmd hg up -q -C .
  (4b6cc7d)

Test out-of-date bookmark
  $ echo rebase > .hg/bookmarks.current
  $ cmd hg book
     active                    2:4b6cc7d5194b
     rebase                    1:775bfdddc842
  (rebase|UPDATE_NEEDED)
  $ hg up -q .

Test remotenames
  $ hg log -r . -T '{node}\n'
  4b6cc7d5194bd5dbf63970015ec75f8fd1de6dba
  $ echo 4b6cc7d5194bd5dbf63970015ec75f8fd1de6dba bookmarks remote/@ > .hg/remotenames
  $ cmd
  (4b6cc7d|remote/@)

Test shared bookmarks
  $ cmd cd ..
  $ cmd hg share -B repo share --config "extensions.share="
  updating working directory
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cmd cd share
  (42eaf5c)
  $ echo rebase > .hg/bookmarks.current
  $ cmd
  (rebase|UPDATE_NEEDED)
  $ cd ../repo

Test unshared bookmarks
  $ cmd cd ..
  $ cmd hg share repo share2 --config "extensions.share="
  updating working directory
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cmd cd share2
  (42eaf5c)
  $ cmd hg book unshared
  (unshared)
  $ cmd hg up -q ".^"
  (4b6cc7d|remote/@)
  $ echo unshared > .hg/bookmarks.current
  $ cmd
  (unshared|remote/@|UPDATE_NEEDED)
  $ cd ../repo

Test branches
  $ cmd hg branch blah
  marked working directory as branch blah
  (branches are permanent and global, did you want a bookmark?)
  (4b6cc7d|remote/@|blah)
  $ cmd hg commit -m blah
  (a742469|blah)
  $ cmd hg up -q default
  (42eaf5c)
  $ cmd hg up -q
  (42eaf5c)

Test branches with bookmarks and other stuff
  $ cmd hg up -q blah
  (a742469|blah)
  $ cmd hg book active
  moving bookmark 'active' forward from 4b6cc7d5194b
  (active|blah)
  $ cmd hg merge 775bfdddc842
  merging b
  warning: conflicts while merging b! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  (active|blah|MERGE)
  $ cmd hg up -q -C default
  (42eaf5c)

Test with symlinks to inside of subdir of repo
  $ mkdir subdir
  $ echo contents > subdir/file
  $ hg add subdir/file
  $ cmd hg commit -m subdir
  (97af35b)
  $ cd ..
  $ cmd ln -s repo/subdir
  $ cmd cd subdir
  (97af35b)
  $ cd ../repo

Test formatting options
  $ _scm_prompt ' %s \n'
   97af35b 
  $ _scm_prompt ':%s:'
  :97af35b: (no-eol)

Test locked repo states (generally due to concurrency so tests are kinda fake)
  $ cmd ln -s "${HOSTNAME}:12345" .hg/wlock
  (97af35b|WDIR-LOCKED)
  $ cmd ln -s "${HOSTNAME}:12345" .hg/store/lock
  (97af35b|STORE-LOCKED)
  $ cmd rm .hg/wlock
  (97af35b|STORE-LOCKED)
  $ cmd rm .hg/store/lock
  (97af35b)

Test many remotenames
  $ hg log -r . -T '{node}\n'
  97af35b3648c0098cbd8114ae1b1bafab997ac20
  $ for i in `$PYTHON $TESTDIR/seq.py 1 10`; do
  > echo 97af35b3648c0098cbd8114ae1b1bafab997ac20 bookmarks remote/remote$i >> .hg/remotenames
  > done
  $ cmd
  (97af35b|remote/remote9...)
  $ echo 97af35b3648c0098cbd8114ae1b1bafab997ac20 bookmarks remote/abc/master >> .hg/remotenames
  $ cmd
  (97af35b|remote/remote9...)
  $ echo 97af35b3648c0098cbd8114ae1b1bafab997ac20 bookmarks remote/@ >> .hg/remotenames
  $ cmd
  (97af35b|remote/@...)

Test eden snapshots
  $ mkdir -p .eden/client/
  $ python -c 'print("eden\x00\x00\x00\x01\xde\xad\xbe\xef\xca\xfe\xfa\xce\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00")' > .eden/client/SNAPSHOT
  $ rm .hg/dirstate
  $ cmd
  (deadbee)
