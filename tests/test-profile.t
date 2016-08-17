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

  $ cd ..
