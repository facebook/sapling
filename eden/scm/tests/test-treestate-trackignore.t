#debugruntest-compatible
#require fsmonitor no-eden

  $ configure modernclient
  $ newclientrepo repo
  $ cat >> .gitignore << EOF
  > .gitignore
  > EOF

Update dirstate initially or next "status" won't trigger migration
  $ hg status

Start tracking ignored files adds them to treestate. The migration only happens once.

  $ setconfig fsmonitor.track-ignore-files=1
  $ LOG=workingcopy::filesystem::watchmanfs=info hg status 2>&1 | grep track-ignored
   INFO pending_changes: workingcopy::filesystem::watchmanfs::watchmanfs: migrating track-ignored track_ignored="1"
  $ LOG=workingcopy::filesystem::watchmanfs=info hg status 2>&1 | grep track-ignored || true
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 

Stop tracking ignored files removes them from treestate. The migration only happens once.

  $ setconfig fsmonitor.track-ignore-files=0
  $ LOG=workingcopy::filesystem::watchmanfs=info hg status 2>&1 | grep track-ignored
   INFO pending_changes: workingcopy::filesystem::watchmanfs::watchmanfs: migrating track-ignored track_ignored="0"
  $ LOG=workingcopy::filesystem::watchmanfs=info hg status 2>&1 | grep track-ignored || true
