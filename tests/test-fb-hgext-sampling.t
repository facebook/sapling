Setup. SCM_SAMPLING_FILEPATH needs to be cleared as some environments may
have it set.

  $ unset SCM_SAMPLING_FILEPATH

  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }
Init the repo
  $ hg init testrepo
  $ cd testrepo
  $ mkcommit a
Create an extension that logs every commit and also call repo.revs twice

Create an extension that logs the call to commit
  $ cat > $TESTTMP/logcommit.py << EOF
  > from mercurial import extensions, localrepo
  > def cb(sample):
  >   return len(sample)
  > def _commit(orig, repo, *args, **kwargs):
  >     repo.ui.log("commit", "match filter", k=1, a={"hi":"ho"})
  >     repo.ui.log("foo", "does not match filter", k=1, a={"hi":"ho"})
  >     repo.ui.log("commit", "message %s", "string", k=1, a={"hi":"ho"})
  >     return orig(repo, *args, **kwargs)
  > def extsetup(ui):
  >     extensions.wrapfunction(localrepo.localrepository, 'commit', _commit)
  >     @ui.atexit
  >     def handler():
  >       ui._measuredtimes['atexit_measured'] += 7
  >       ui.warn("atexit handler executed\n")
  > EOF


Set up the extension and set a log file
We whitelist only the 'commit' key, only the events with that key will be
logged
  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logmeasuredtimes=True
  > [sampling]
  > key.commit=commit_table
  > key.measuredtimes=measuredtimes
  > [extensions]
  > sampling=
  > EOF
  $ LOGDIR=`pwd`/logs
  $ mkdir $LOGDIR
  $ echo "logcommit=$TESTTMP/logcommit.py" >> $HGRCPATH
  $ echo "[sampling]" >> $HGRCPATH
  $ echo "filepath = $LOGDIR/samplingpath.txt" >> $HGRCPATH

Do a couple of commits.  We expect to log two messages per call to repo.commit.
  $ mkdir a_topdir && cd a_topdir
  $ mkcommit b
  atexit handler executed
  atexit handler executed
  $ mkcommit c
  atexit handler executed
  atexit handler executed
  >>> import json
  >>> with open("$LOGDIR/samplingpath.txt") as f:
  ...     data = f.read()
  >>> for record in data.strip("\0").split("\0"):
  ...     parsedrecord = json.loads(record)
  ...     if parsedrecord['category'] == 'commit_table':
  ...         print(' '.join([parsedrecord["data"]["msg"], parsedrecord["category"]]))
  ...         assert len(parsedrecord["data"]) == 4
  ...     elif parsedrecord['category'] == 'measuredtimes':
  ...         print('atexit_measured: ', repr(sorted(parsedrecord['data'])))
  atexit_measured:  [u'atexit_measured', u'command_duration', u'dirstatewalk_time', u'metrics_type', u'msg', u'stdio_blocked'] (no-fsmonitor !)
  atexit_measured:  [u'atexit_measured', u'command_duration', u'fsmonitorwalk_time', u'metrics_type', u'msg', u'stdio_blocked'] (fsmonitor !)
  match filter commit_table
  message string commit_table
  atexit_measured:  [u'atexit_measured', u'command_duration', u'dirstatewalk_time', u'metrics_type', u'msg', u'stdio_blocked'] (no-fsmonitor !)
  atexit_measured:  [u'atexit_measured', u'command_duration', u'dirstatewalk_time', u'metrics_type', u'msg', u'stdio_blocked'] (no-fsmonitor !)
  atexit_measured:  [u'atexit_measured', u'command_duration', u'fsmonitorwalk_time', u'metrics_type', u'msg', u'stdio_blocked', u'watchmanquery_time'] (fsmonitor !)
  atexit_measured:  [u'atexit_measured', u'command_duration', u'fsmonitorwalk_time', u'metrics_type', u'msg', u'stdio_blocked'] (fsmonitor !)
  match filter commit_table
  message string commit_table
  atexit_measured:  [u'atexit_measured', u'command_duration', u'dirstatewalk_time', u'metrics_type', u'msg', u'stdio_blocked'] (no-fsmonitor !)
  atexit_measured:  [u'atexit_measured', u'command_duration', u'fsmonitorwalk_time', u'metrics_type', u'msg', u'stdio_blocked', u'watchmanquery_time'] (fsmonitor !)

Test topdir logging:
  $ setconfig sampling.logtopdir=True
  $ setconfig sampling.key.command_info=command_info
  $ hg st > /dev/null
  atexit handler executed
  >>> import json
  >>> with open("$LOGDIR/samplingpath.txt") as f:
  ...     data = f.read().strip("\0").split("\0")
  >>> print([json.loads(d)["data"]["topdir"] for d in data if "topdir" in d])
  [u'a_topdir']
