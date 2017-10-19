test --time

  $ hg --time help -q help 2>&1 | grep time > /dev/null
  $ hg init a
  $ cd a

Function to check that statprof ran
  $ statprofran () {
  >   egrep 'Sample count:|No samples recorded' > /dev/null
  > }

test --profile

  $ hg st --profile 2>&1 | statprofran

Abreviated version

  $ hg st --prof 2>&1 | statprofran

In alias

  $ hg --config "alias.profst=status --profile" profst 2>&1 | statprofran

#if lsprof

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
  > from mercurial import registrar, commands
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'sleep', [], 'hg sleep')
  > def sleep(ui, *args, **kwargs):
  >     time.sleep(0.1)
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > sleep = `pwd`/sleepext.py
  > EOF

statistical profiler works

  $ hg --profile sleep 2>../out
  $ cat ../out | statprofran

Various statprof formatters work

  $ hg --profile --config profiling.statformat=byline sleep 2>../out
  $ head -n 1 ../out
    %   cumulative      self          
  $ cat ../out | statprofran

  $ hg --profile --config profiling.statformat=bymethod sleep 2>../out
  $ head -n 1 ../out
    %   cumulative      self          
  $ cat ../out | statprofran

  $ hg --profile --config profiling.statformat=hotpath sleep 2>../out
  $ cat ../out | statprofran

  $ hg --profile --config profiling.statformat=json sleep 2>../out
  $ cat ../out
  \[\[-?\d+.* (re)

statprof can be used as a standalone module

  $ $PYTHON -m mercurial.statprof hotpath
  must specify --file to load
  [1]

  $ cd ..

#if no-chg
profiler extension could be loaded before other extensions

  $ cat > fooprof.py <<EOF
  > from __future__ import absolute_import
  > import contextlib
  > @contextlib.contextmanager
  > def profile(ui, fp):
  >     print('fooprof: start profile')
  >     yield
  >     print('fooprof: end profile')
  > def extsetup(ui):
  >     ui.write('fooprof: loaded\n')
  > EOF

  $ cat > otherextension.py <<EOF
  > from __future__ import absolute_import
  > def extsetup(ui):
  >     ui.write('otherextension: loaded\n')
  > EOF

  $ hg init b
  $ cd b
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > other = $TESTTMP/otherextension.py
  > fooprof = $TESTTMP/fooprof.py
  > EOF

  $ hg root
  otherextension: loaded
  fooprof: loaded
  $TESTTMP/b (glob)
  $ HGPROF=fooprof hg root --profile
  fooprof: loaded
  fooprof: start profile
  otherextension: loaded
  $TESTTMP/b (glob)
  fooprof: end profile

  $ HGPROF=other hg root --profile 2>&1 | head -n 2
  otherextension: loaded
  unrecognized profiler 'other' - ignored

  $ HGPROF=unknown hg root --profile 2>&1 | head -n 1
  unrecognized profiler 'unknown' - ignored

  $ cd ..
#endif
