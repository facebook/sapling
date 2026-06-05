  $ eagerepo
test --time

  $ sl --time help -q help 2>&1 | grep time > /dev/null
  $ sl init a
  $ cd a

Function to check that statprof ran
  $ statprofran () {
  >   grep -E 'Sample count:|No samples recorded' > /dev/null
  > }

Test python profiling

  $ sl log -r . --config profiling.enabled-python=true 2>&1 | statprofran

Affects alias

  $ sl --config "alias.proflog=log -r ."  --config profiling.enabled-python=true proflog 2>&1 | statprofran

#if normal-layout
statprof can be used as a standalone module

  $ sl debugpython -- -m sapling.statprof hotpath
  must specify --file to load
  [1]
#endif

  $ cd ..

Test minelapsed config option
(This cannot be tested because profiling is disabled for 'debugshell')

Test other config sections

  $ sl --config profiling:background.enabled-python=1 --config profiling:background.output=z debugshell -c '1'
  unrecognized profiler 'None' - ignored
  invalid sampling frequency 'None' - ignoring
  unknown profiler output format: None
  $ [ -f z ]

  $ sl --config profiling.enabled-python=1 --config profiling.output=x --config profiling:background.enabled-python=1 --config profiling:background.output=y debugshell -c '1'
  $ [ -f x ]
  $ [ -f y ]
  [1]

Test statprof will not take at least frequency time.

  >>> import time
  >>> _ = open('start', 'w').write('%s' % time.time())

  $ sl --profile --config profiling.output=z --config profiling.type=stat --config profiling.freq=0.02 debugshell -c 'a=1'

  >>> import time
  >>> time.time() - float(open('start').read()) < 50
  True
