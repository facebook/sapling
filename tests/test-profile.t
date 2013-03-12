test --time

  $ hg --time help -q help 2>&1 | grep time > /dev/null
  $ hg init a
  $ cd a

#if lsprof

test --profile

  $ hg --profile st 2>../out
  $ grep CallCount ../out > /dev/null || cat ../out

  $ hg --profile --config profiling.output=../out st
  $ grep CallCount ../out > /dev/null || cat ../out

  $ hg --profile --config profiling.format=text st 2>../out
  $ grep CallCount ../out > /dev/null || cat ../out

  $ echo "[profiling]" >> $HGRCPATH
  $ echo "format=kcachegrind" >> $HGRCPATH

  $ hg --profile st 2>../out
  $ grep 'events: Ticks' ../out > /dev/null || cat ../out

  $ hg --profile --config profiling.output=../out st
  $ grep 'events: Ticks' ../out > /dev/null || cat ../out

#endif

  $ cd ..
