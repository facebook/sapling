#require bash no-eden

To run this test against other shells, use the shell argument, eg:
run-tests.py --shell=zsh test-scm-prompt*

  $ configure mutation-norecord

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
  $ newclientrepo repo
  $ echo a > a
  $ cmd sl add a
  (0000000000)
  $ cmd sl commit -m 'c1'
  (5cad84d172)
  $ cmd sl book active
  (active)
  $ cmd sl book -i
  (5cad84d172)
  $ echo b > b
  $ cmd sl add b
  (5cad84d172)
  $ cmd sl commit -m 'c2'
  (775bfdddc8)
  $ cmd sl up -q active
  (active)
  $ echo bb > b
  $ sl add b
  $ cmd sl commit -m 'c3'
  (active)
  $ sl log -T '{node|short} {desc}\n' -G
  @  4b6cc7d5194b c3
  │
  │ o  775bfdddc842 c2
  ├─╯
  o  5cad84d1726f c1
  

Test rebase
  $ cmd sl rebase -d 775bfdd --config "extensions.rebase="
  rebasing 4b6cc7d5194b "c3" (active)
  merging b
  warning: 1 conflicts while merging b! (edit, then use 'sl resolve --mark')
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  (775bfdddc8|REBASE)
  $ cmd sl book rebase
  (rebase|REBASE)
  $ cmd sl rebase --abort --config "extensions.rebase="
  rebase aborted
  (active)
  $ cmd sl book -i
  (4b6cc7d519)

Test histedit
  $ command cat > commands <<EOF
  > edit 4b6cc7d5194b
  > EOF
  $ cmd sl histedit --config "extensions.histedit=" --commands commands
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  adding b
  Editing (4b6cc7d5194b), you may commit or record as needed now.
  (sl histedit --continue to resume)
  (5cad84d172|HISTEDIT)
  $ cmd sl histedit --config "extensions.histedit=" --abort
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (4b6cc7d519)

Test graft
  $ cmd sl graft 775bfdddc842
  grafting 775bfdddc842 "c2" (rebase)
  merging b
  warning: 1 conflicts while merging b! (edit, then use 'sl resolve --mark')
  abort: unresolved conflicts, can't continue
  (use 'sl resolve' and 'sl graft --continue')
  (4b6cc7d519|GRAFT)
  $ cmd sl revert -r 775bfdddc842 b
  (4b6cc7d519|GRAFT)
  $ cmd sl resolve --mark b
  (no more unresolved files)
  continue: sl graft --continue
  (4b6cc7d519|GRAFT)
  $ cmd sl graft --continue
  grafting 775bfdddc842 "c2" (rebase)
  (42eaf5ca82)

Test bisect
  $ cmd sl bisect -b .
  (42eaf5ca82|BISECT)
  $ cmd sl bisect -g ".^^"
  Testing changeset 4b6cc7d5194b (2 changesets remaining, ~1 tests)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (4b6cc7d519|BISECT)
  $ cmd sl bisect -r
  (4b6cc7d519)

Test unshelve
  $ echo b >> b
  $ cmd sl shelve --config "extensions.shelve="
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (4b6cc7d519)
  $ cmd sl up -q ".^"
  (5cad84d172)
  $ cmd sl unshelve --config "extensions.shelve="
  unshelving change 'default'
  rebasing shelved changes
  rebasing 19f7fec7f80b "shelve changes to: c3"
  other [source] changed b which local [dest] is missing
  hint: the missing file was probably added by commit 4b6cc7d5194b in the branch being rebased
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see 'sl resolve', then 'sl unshelve --continue')
  (5cad84d172|UNSHELVE)
  $ cmd sl unshelve --config "extensions.shelve=" --abort
  rebase aborted
  unshelve of 'default' aborted
  (5cad84d172)

Test merge
  $ cmd sl up -q 4b6cc7d5194b
  (4b6cc7d519)
  $ cmd sl merge 775bfdddc842
  merging b
  warning: 1 conflicts while merging b! (edit, then use 'sl resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  (4b6cc7d519|MERGE)
  $ cmd sl up -q -C .
  (4b6cc7d519)

Test out-of-date bookmark
  $ echo rebase > .sl/bookmarks.current
  $ cmd sl book
     active                    4b6cc7d5194b
     rebase                    775bfdddc842
  (rebase|UPDATE_NEEDED)
  $ sl up -q .

Test remotenames
  $ sl log -r . -T '{node}\n'
  4b6cc7d5194bd5dbf63970015ec75f8fd1de6dba
  $ echo 4b6cc7d5194bd5dbf63970015ec75f8fd1de6dba bookmarks remote/@ > .sl/store/remotenames
  $ cmd
  (4b6cc7d519|remote/@)

Test with symlinks to inside of subdir of repo
  $ mkdir subdir
  $ echo contents > subdir/file
  $ sl add subdir/file
  $ cmd sl commit -m subdir
  (4c449fd971)
  $ cd ..
  $ cmd ln -s repo/subdir
  $ cmd cd subdir
  (4c449fd971)
  $ cd ../repo

Test formatting options
  $ _scm_prompt ' %s \n'
   4c449fd971 
  $ _scm_prompt ':%s:'
  :4c449fd971: (no-eol)

Test locked repo states (generally due to concurrency so tests are kinda fake)
  $ cmd ln -s "${HOSTNAME}:12345" .sl/wlock
  (4c449fd971|WDIR-LOCKED)
  $ cmd ln -s "${HOSTNAME}:12345" .sl/store/lock
  (4c449fd971|STORE-LOCKED)
  $ cmd rm .sl/wlock
  (4c449fd971|STORE-LOCKED)
  $ cmd rm .sl/store/lock
  (4c449fd971)

Test many remotenames
  $ sl log -r . -T '{node}\n'
  4c449fd97125b3e1dafad3e702a521194c14672a
  $ for i in `seq 1 10`; do
  > echo 4c449fd97125b3e1dafad3e702a521194c14672a bookmarks remote/remote$i >> .sl/store/remotenames
  > done
  $ cmd
  (4c449fd971|remote/remote9...)
  $ echo 97af35b3648c0098cbd8114ae1b1bafab997ac20 bookmarks remote/abc/master >> .sl/store/remotenames
  $ cmd
  (4c449fd971|remote/remote9...)
  $ echo 97af35b3648c0098cbd8114ae1b1bafab997ac20 bookmarks remote/@ >> .sl/store/remotenames
  $ cmd
  (4c449fd971|remote/remote9...)

Test worktreename marker (.sl/worktreename) gated on SCM_PROMPT_SHOW_WORKTREE.
The marker is auto-written (no trailing newline) by `sl worktree add` and
`sl worktree label`; this test simulates that with `printf > .sl/worktreename`.

Without marker, env var is a no-op (covers main checkouts and non-EdenFS sl)
  $ test -e .sl/worktreename
  [1]
  $ export SCM_PROMPT_SHOW_WORKTREE=1
  $ cmd
  (4c449fd971|remote/remote9...)
  $ unset SCM_PROMPT_SHOW_WORKTREE

With marker present, env var gates display
  $ printf feature1 > .sl/worktreename
  $ cmd
  (4c449fd971|remote/remote9...)
  $ export SCM_PROMPT_SHOW_WORKTREE=1
  $ cmd
  (4c449fd971|feature1|remote/remote9...)

Empty marker file produces no suffix
  $ printf '' > .sl/worktreename
  $ cmd
  (4c449fd971|remote/remote9...)
  $ rm .sl/worktreename
  $ cmd
  (4c449fd971|remote/remote9...)
  $ unset SCM_PROMPT_SHOW_WORKTREE
