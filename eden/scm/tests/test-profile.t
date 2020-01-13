test --time

  $ hg --time help -q help 2>&1 | grep time > /dev/null
  $ hg init a
  $ cd a

Function to check that statprof ran
  $ statprofran () {
  >   egrep 'Sample count:|No samples recorded' > /dev/null
  > }

test --profile

  $ hg log -r . --profile 2>&1 | statprofran

Abreviated version

  $ hg log -r . --prof 2>&1 | statprofran

In alias

  $ hg --config "alias.proflog=log -r . --profile" proflog 2>&1 | statprofran

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
