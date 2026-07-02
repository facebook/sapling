#debugruntest-incompatible

#require bash no-eden

  $ configure mutation-norecord

Initialize scm prompt
  $ . $TESTDIR/../contrib/scm-prompt.sh

  $ cmd() {
  >   "$@"
  >   _scm_prompt "(%s)\n"
  > }

Set up source repo with a commit and a remotename
  $ newclientrepo source
  $ echo a > a
  $ cmd sl add a
  (0000000000)
  $ cmd sl commit -m 'c1'
  (5cad84d172)
  $ NODE=$(sl log -r . -T '{node}\n')
  $ echo "$NODE bookmarks remote/@" > .sl/store/remotenames
  $ cmd
  (5cad84d172|remote/@)
  $ cd ..

Absolute share: cd into the shared checkout, prompt should show the same remotename
  $ sl --config "extensions.share=" share source shared --noupdate -q
  $ cd shared
  $ sl up -q $NODE
  $ _scm_prompt "(%s)\n"
  (5cad84d172|remote/@)

Per-worktree state still reads from local .sl, even when shared
  $ cmd ln -s "${HOSTNAME}:12345" .sl/wlock
  (5cad84d172|remote/@|WDIR-LOCKED)
  $ rm .sl/wlock
  $ _scm_prompt "(%s)\n"
  (5cad84d172|remote/@)
  $ cd ..

Relative share: this is the case the §1 fix unblocks. Without the fix, the relative
sharedpath wouldn't resolve and remotenames would silently disappear from the prompt.
  $ sl --config "extensions.share=" share --relative source shared-rel --noupdate -q
  $ cd shared-rel
  $ sl up -q $NODE
  $ _scm_prompt "(%s)\n"
  (5cad84d172|remote/@)

UPDATE_NEEDED suffix: bookmark in shared store points to a different commit than dirstate
  $ sl book mybook -r $NODE
  $ echo mybook > .sl/bookmarks.current
  $ sl commit --config "ui.allowemptycommit=true" -m 'c2'
  $ _scm_prompt "(%s)\n"
  (mybook|UPDATE_NEEDED)
