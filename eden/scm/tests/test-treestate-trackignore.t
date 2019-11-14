#require fsmonitor

  $ newrepo
  $ cat >> .gitignore << EOF
  > .gitignore
  > EOF

  $ hg status
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 

Stop tracking ignored files removes them from treestate. The migration only happens once.

  $ setconfig fsmonitor.track-ignore-files=0
  $ hg status --debug | grep tracking
  stop tracking ignored files
  $ hg status
  $ hg debugtree list

Start tracking ignored files adds them to treestate. The migration only happens once.

  $ setconfig fsmonitor.track-ignore-files=1
  $ hg status --debug | grep tracking
  start tracking 1 ignored files
  $ hg status
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 
