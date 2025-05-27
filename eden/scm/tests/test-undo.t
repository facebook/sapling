
#require no-eden


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

  $ hg undo --keep
  undone to *, before commit -Aqm initial commit (glob)

  $ hg log -r 'olddraft(0)'
  commit:      79344ac2ab8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     initial commit

  $ hg status
  M file

  $ hg commit -Aqm "recommit after undo --keep"
  $ hg log -r 'olddraft(0)'
  commit:      79344ac2ab8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     initial commit
  
  commit:      0109d94c2173
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     recommit after undo --keep


  $ hg undo
  undone to *, before commit -Aqm recommit after undo --keep (glob)
  hint[undo-uncommit-unamend]: undoing commits discards their changes.
  to restore the changes to the working copy, run 'hg revert -r 0109d94c2173 --all'
  in the future, you can use 'hg uncommit' instead of 'hg undo' to keep changes
  hint[hint-ack]: use 'hg hint --ack undo-uncommit-unamend' to silence these hints

  $ hg undo foo
  hg undo: invalid arguments
  (use 'hg undo -h' to get help)
  [255]



