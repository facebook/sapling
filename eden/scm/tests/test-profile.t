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
  $ hg blackbox --pattern '{"profile":"_"}' | grep -o CallCount
  CallCount

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
  > from edenscm.mercurial import registrar, commands
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

#if normal-layout
statprof can be used as a standalone module

  $ hg debugpython -- -m edenscm.mercurial.statprof hotpath
  must specify --file to load
  [1]
#endif

  $ cd ..

#if no-chg
profiler extension could be loaded before other extensions

  $ cat > fooprof.py <<EOF
  > from __future__ import absolute_import
  > import contextlib
  > @contextlib.contextmanager
  > def profile(ui, fp, section):
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

  $ hg log -r null -T "foo\n"
  otherextension: loaded
  fooprof: loaded
  foo
  $ HGPROF=fooprof hg log -r null -T "foo\n" --profile
  fooprof: loaded
  fooprof: start profile
  otherextension: loaded
  foo
  fooprof: end profile

  $ HGPROF=other hg log -r null -T "foo\n" --profile 2>&1 | head -n 2
  otherextension: loaded
  unrecognized profiler 'other' - ignored

  $ HGPROF=unknown hg log -r null -T "foo\n" --profile 2>&1 | head -n 1
  unrecognized profiler 'unknown' - ignored

  $ cd ..
#endif

Test minelapsed config option

  $ hg --profile --config profiling.minelapsed=1000 debugshell -c 'ui.write("1\n")'
  1
  $ hg --profile --config profiling.minelapsed=1 debugshell -c 'import time; time.sleep(1.1)' 2>&1 | grep Sample
  Sample count: * (glob)

Test other config sections

  $ hg --config profiling:background.enabled=1 --config profiling:background.output=z debugshell -c '1'
  unrecognized profiler 'None' - ignored
  invalid sampling frequency 'None' - ignoring
  unknown profiler output format: None
  $ [ -f z ]

  $ hg --profile --config profiling.output=x --config profiling:background.enabled=1 --config profiling:background.output=y debugshell -c '1'
  $ [ -f x ]
  $ [ -f y ]
  [1]

Test statprof will not take at least frequency time.

  >>> import time
  >>> open('start', 'w').write('%s' % time.time())

  $ hg --profile --config profiling.output=z --config profiling.type=stat --config profiling.freq=0.02 debugshell -c 'a=1'

  >>> import time
  >>> time.time() - float(open('start').read()) < 50
  True
