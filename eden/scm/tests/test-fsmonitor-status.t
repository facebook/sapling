#require fsmonitor

  $ configure modernclient
  $ newclientrepo repo
  $ echo >> file1
  $ echo >> file2
  $ echo >> file3
  $ hg commit -Aqm base
  $ setconfig fsmonitor.watchman-changed-file-threshold=3
  $ hg status

# Editing a new file causes a treestate flush.
  $ echo >> file1
  $ EDENSCM_LOG=workingcopy::watchmanfs::treestate=debug hg status
  DEBUG workingcopy::watchmanfs::treestate: flushing dirty treestate
  M file1

# A second status does not cause a treestate flush.
  $ EDENSCM_LOG=workingcopy::watchmanfs::treestate=debug hg status
  DEBUG workingcopy::watchmanfs::treestate: skipping treestate flush - it is not dirty
  M file1

# Editing an already edited file does not cause a treestate flush.
  $ echo >> file1
  $ EDENSCM_LOG=workingcopy::watchmanfs::treestate=debug hg status
  DEBUG workingcopy::watchmanfs::treestate: skipping treestate flush - it is not dirty
  M file1

# Reverting a file to clean does cause a treestate flush.
  $ echo > file1
  $ EDENSCM_LOG=workingcopy::watchmanfs::treestate=debug hg status
  DEBUG workingcopy::watchmanfs::treestate: flushing dirty treestate

# Setup 3 edits for the next test
  $ echo >> file1
  $ echo >> file2
  $ echo >> file3
  $ EDENSCM_LOG=workingcopy::watchmanfs::treestate=debug hg status
  DEBUG workingcopy::watchmanfs::treestate: flushing dirty treestate
  M file1
  M file2
  M file3

# Editing less than watchman-changed-file-threshold files does not cause a
# treestate flush.
  $ echo >> file1
  $ echo >> file2
  $ EDENSCM_LOG=workingcopy::watchmanfs::treestate=debug hg status
  DEBUG workingcopy::watchmanfs::treestate: skipping treestate flush - it is not dirty
  M file1
  M file2
  M file3

# Editing more than watchman-changed-file-threshold files causes a dirstate
# flush because the clock is updated, even if the files were already considered
# edited.
  $ echo >> file3
  $ EDENSCM_LOG=workingcopy::watchmanfs::treestate=debug hg status
  DEBUG workingcopy::watchmanfs::treestate: flushing dirty treestate
  M file1
  M file2
  M file3
