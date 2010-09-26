test --time

  $ hg --time help -q help 2>&1 | grep Time > /dev/null
  $ hg init a
  $ cd a

test --profile

  $ if "$TESTDIR/hghave" -q lsprof; then
  >     hg --profile st 2>../out || echo --profile failed
  >     grep CallCount < ../out > /dev/null || echo wrong --profile
  > 
  >     hg --profile --config profiling.output=../out st 2>&1 \
  >         || echo --profile + output to file failed
  >     grep CallCount < ../out > /dev/null \
  >         || echo wrong --profile output when saving to a file
  > 
  >     hg --profile --config profiling.format=text st 2>&1 \
  >         | grep CallCount > /dev/null || echo --profile format=text failed
  > 
  >     echo "[profiling]" >> $HGRCPATH
  >     echo "format=kcachegrind" >> $HGRCPATH
  > 
  >     hg --profile st 2>../out || echo --profile format=kcachegrind failed
  >     grep 'events: Ticks' < ../out > /dev/null || echo --profile output is wrong
  > 
  >     hg --profile --config profiling.output=../out st 2>&1 \
  >         || echo --profile format=kcachegrind + output to file failed
  >     grep 'events: Ticks' < ../out > /dev/null \
  >         || echo --profile output is wrong
  > fi
