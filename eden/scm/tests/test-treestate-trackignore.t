#debugruntest-compatible
#require fsmonitor

  $ configure modernclient
# trackignore functionality is not used in production anymore, so we can
# probably delete this test once we fully migrated to Rust status.
  $ setconfig status.use-rust=False workingcopy.ruststatus=False
  $ newclientrepo repo
  $ cat >> .gitignore << EOF
  > .gitignore
  > EOF

Update dirstate initially or next "status" won't trigger migration
  $ hg status

  $ setconfig fsmonitor.track-ignore-files=1
  $ hg status --debug 2>&1 | grep tracking
  start tracking 1 ignored files
  $ hg status --debug 2>&1 | grep tracking || true
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 

Stop tracking ignored files removes them from treestate. The migration only happens once.

  $ setconfig fsmonitor.track-ignore-files=0
  $ hg status --debug 2>&1 | grep tracking
  stop tracking ignored files
  $ hg status --debug 2>&1 | grep tracking || true

Make sure Rust status doesn't track ignored files
  $ hg dbsh << 'EOS'
  > watchman_command = repo._watchmanclient.command
  > # Simulate watchman restart
  > watchman_command('watch-del-all')
  > EOS
  $ hg status --config status.use-rust=true

  $ hg debugtree list

Start tracking ignored files adds them to treestate. The migration only happens once.

  $ setconfig fsmonitor.track-ignore-files=1
  $ hg status --debug 2>&1 | grep tracking
  start tracking 1 ignored files
  $ hg status --debug 2>&1 | grep tracking || true
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 

Rust status can also migrate:
  $ setconfig status.use-rust=true workingcopy.use-rust=true

  $ setconfig fsmonitor.track-ignore-files=false
  $ LOG=workingcopy=info hg status 2>&1 | grep migrating
   INFO pending_changes: workingcopy::watchmanfs::watchmanfs: migrating track-ignored track_ignored="0"
  $ LOG=workingcopy=info hg status 2>&1 | grep migrating || true
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 

Rust status can migrate back:
  $ setconfig fsmonitor.track-ignore-files=true
  $ LOG=workingcopy=info hg status 2>&1 | grep migrating
   INFO pending_changes: workingcopy::watchmanfs::watchmanfs: migrating track-ignored track_ignored="1"
  $ LOG=workingcopy=info hg status 2>&1 | grep migrating || true
  $ hg debugtree list
  .gitignore: 0666 -1 -1 NEED_CHECK 
