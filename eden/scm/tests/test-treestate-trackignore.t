#require fsmonitor

  $ configure modernclient
  $ setconfig status.use-rust=False
  $ newclientrepo repo
  $ cat >> .gitignore << EOF
  > .gitignore
  > EOF

  $ hg status
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 

Stop tracking ignored files removes them from treestate. The migration only happens once.

  $ setconfig fsmonitor.track-ignore-files=0
  $ hg status --debug 2>&1 | grep tracking
  stop tracking ignored files
  $ hg status
  $ hg debugtree list

Start tracking ignored files adds them to treestate. The migration only happens once.

  $ setconfig fsmonitor.track-ignore-files=1
  $ hg status --debug 2>&1 | grep tracking
  start tracking 1 ignored files
  $ hg status
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 
