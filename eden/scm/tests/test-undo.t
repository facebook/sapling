#chg-compatible

  $ configure modernclient
  $ newclientrepo repo
  $ enable undo
  $ echo >> file
  $ hg commit -Aqm "initial commit"
  $ echo >> file
  $ hg commit -Aqm "initial commit"
  $ hg log -r 'olddraft(0)'
  commit:      79344ac2ab8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     initial commit
  
  commit:      f5a897cc70a1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     initial commit
  
  $ hg log -r 'oldworkingcopyparent(0)'
  commit:      f5a897cc70a1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     initial commit
  
  $ hg log -r . -T '{undonecommits(0)}\n'
  
  $ hg log -r . -T '{donecommits(0)}\n'
  f5a897cc70a18acf06b00febe9ad748ac761067d
  $ hg log -r . -T '{oldworkingcopyparent(0)}\n'
  f5a897cc70a18acf06b00febe9ad748ac761067d
  $ hg undo --preview
  @
  │
  o
  
  undo to *, before commit -Aqm initial commit (glob)

  $ hg undo
  undone to *, before commit -Aqm initial commit (glob)
  hint[undo-uncommit-unamend]: undoing commits discards their changes.
  to restore the changes to the working copy, run 'hg revert -r f5a897cc70a1 --all'
  in the future, you can use 'hg uncommit' instead of 'hg undo' to keep changes
  hint[hint-ack]: use 'hg hint --ack undo-uncommit-unamend' to silence these hints


