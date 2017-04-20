test --time

  $ hg --time help -q help 2>&1 | grep time > /dev/null
  $ hg init a
  $ cd a

#if lsprof

test --profile

  $ prof='hg --config profiling.type=ls --profile'

  $ $prof st 2>../out
  $ grep CallCount ../out > /dev/null || cat ../out

  $ $prof --config profiling.output=../out st
  $ grep CallCount ../out > /dev/null || cat ../out

  $ $prof --config profiling.output=blackbox --config extensions.blackbox= st
  $ grep CallCount .hg/blackbox.log > /dev/null || cat .hg/blackbox.log

  $ $prof --config profiling.format=text st 2>../out
  $ grep CallCount ../out > /dev/null || cat ../out

  $ echo "[profiling]" >> $HGRCPATH
  $ echo "format=kcachegrind" >> $HGRCPATH

  $ $prof st 2>../out
  $ grep 'events: Ticks' ../out > /dev/null || cat ../out

  $ $prof --config profiling.output=../out st
  $ grep 'events: Ticks' ../out > /dev/null || cat ../out

#endif

#if lsprof serve

Profiling of HTTP requests works

  $ $prof --config profiling.format=text --config profiling.output=../profile.log serve -d -p $HGPORT --pid-file ../hg.pid -A ../access.log
  $ cat ../hg.pid >> $DAEMON_PIDS
  $ hg -q clone -U http://localhost:$HGPORT ../clone

A single profile is logged because file logging doesn't append
  $ grep CallCount ../profile.log | wc -l
  \s*1 (re)

#endif

Install an extension that can sleep and guarantee a profiler has time to run

  $ cat >> sleepext.py << EOF
  > import time
  > from mercurial import cmdutil, commands
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('sleep', [], 'hg sleep')
  > def sleep(ui, *args, **kwargs):
  >     time.sleep(0.1)
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > sleep = `pwd`/sleepext.py
  > EOF

statistical profiler works

  $ hg --profile sleep 2>../out
  $ grep Sample ../out
  Sample count: \d+ (re)

Various statprof formatters work

  $ hg --profile --config profiling.statformat=byline sleep 2>../out
  $ head -n 1 ../out
    %   cumulative      self          
  $ grep Sample ../out
  Sample count: \d+ (re)

  $ hg --profile --config profiling.statformat=bymethod sleep 2>../out
  $ head -n 1 ../out
    %   cumulative      self          
  $ grep Sample ../out
  Sample count: \d+ (re)

  $ hg --profile --config profiling.statformat=hotpath sleep 2>../out
  $ grep Sample ../out
  Sample count: \d+ (re)

  $ hg --profile --config profiling.statformat=json sleep 2>../out
  $ cat ../out
  \[\[-?\d+.* (re)

statprof can be used as a standalone module

  $ $PYTHON -m mercurial.statprof hotpath
  must specify --file to load
  [1]

  $ cd ..
