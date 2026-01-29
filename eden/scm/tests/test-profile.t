#debugruntest-incompatible
  $ eagerepo
test --time

  $ hg --time help -q help 2>&1 | grep time > /dev/null
  $ hg init a
  $ cd a

Function to check that statprof ran
  $ statprofran () {
  >   grep -E 'Sample count:|No samples recorded' > /dev/null
  > }

test --profile

  $ hg log -r . --profile 2>&1 | statprofran

Abreviated version

  $ hg log -r . --prof 2>&1 | statprofran

In alias

  $ hg --config "alias.proflog=log -r . --profile" proflog 2>&1 | statprofran

#if normal-layout
statprof can be used as a standalone module

  $ hg debugpython -- -m sapling.statprof hotpath
  must specify --file to load
  [1]
#endif

  $ cd ..

Test minelapsed config option
(This cannot be tested because profiling is disabled for 'debugshell')

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
  >>> _ = open('start', 'w').write('%s' % time.time())

  $ hg --profile --config profiling.output=z --config profiling.type=stat --config profiling.freq=0.02 debugshell -c 'a=1'

  >>> import time
  >>> time.time() - float(open('start').read()) < 50
  True
