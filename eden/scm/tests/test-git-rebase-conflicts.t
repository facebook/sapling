#chg-compatible
#require git no-windows
#debugruntest-compatible

Test that rebasing in a git repo with conflicts work.

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true
  $ enable rebase

Prepare the repo

  $ hg init --git repo1
  $ cd repo1
  $ drawdag << 'EOS'
  >         # B/A=0
  > A7  B   # A5/A=3
  >  : /    # A3/A=2
  >  A1     # A1/A=1
  > EOS

Rebase:

  $ hg rebase -r $B -d $A7
  rebasing 5c2dbc94ad6b "B"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg rebase --abort
  rebase aborted

Rebase with merge.printcandidatecommits:

  $ hg rebase -r $B -d $A7 --config merge.printcandidatecommmits=1
  rebasing 5c2dbc94ad6b "B"
  merging A
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
   2 commits might have introduced this conflict:
    - [ff6d58de9da5] A5
    - [b7b8bbe2022e] A3
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
