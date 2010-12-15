
  $ cat > loop.py <<EOF
  > from mercurial import commands
  > 
  > def loop(ui, loops, **opts):
  >     loops = int(loops)
  >     total = None
  >     if loops >= 0:
  >         total = loops
  >     if opts.get('total', None):
  >         total = int(opts.get('total'))
  >     loops = abs(loops)
  > 
  >     for i in range(loops):
  >         ui.progress('loop', i, 'loop.%d' % i, 'loopnum', total)
  >     ui.progress('loop', None, 'loop.done', 'loopnum', total)
  > 
  > commands.norepo += " loop"
  > 
  > cmdtable = {
  >     "loop": (loop, [('', 'total', '', 'override for total')],
  >              'hg loop LOOPS'),
  > }
  > EOF

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "progress=" >> $HGRCPATH
  $ echo "loop=`pwd`/loop.py" >> $HGRCPATH
  $ echo "[progress]" >> $HGRCPATH
  $ echo "assume-tty=1" >> $HGRCPATH
  $ echo "width=60" >> $HGRCPATH

test default params, display nothing because of delay

  $ hg -y loop 3 2>&1 | $TESTDIR/filtercr.py
  
  $ echo "delay=0" >> $HGRCPATH
  $ echo "refresh=0" >> $HGRCPATH

test with delay=0, refresh=0

  $ hg -y loop 3 2>&1 | $TESTDIR/filtercr.py
  
  loop [                                                ] 0/3
  loop [===============>                                ] 1/3
  loop [===============================>                ] 2/3
                                                              \r (esc)

test refresh is taken in account

  $ hg -y --config progress.refresh=100 loop 3 2>&1 | $TESTDIR/filtercr.py
  

test format options 1

  $ hg -y --config 'progress.format=number topic item+2' loop 2 2>&1 \
  > | $TESTDIR/filtercr.py
  
  0/2 loop lo
  1/2 loop lo
                                                              \r (esc)

test format options 2

  $ hg -y --config 'progress.format=number item-3 bar' loop 2 2>&1 \
  > | $TESTDIR/filtercr.py
  
  0/2 p.0 [                                                 ]
  1/2 p.1 [=======================>                         ]
                                                              \r (esc)

test format options and indeterminate progress

  $ hg -y --config 'progress.format=number item bar' loop -- -2 2>&1 \
  > | $TESTDIR/filtercr.py
  
  0 loop.0               [ <=>                              ]
  1 loop.1               [  <=>                             ]
                                                              \r (esc)

make sure things don't fall over if count > total

  $ hg -y loop --total 4 6 2>&1 | $TESTDIR/filtercr.py
  
  loop [                                                ] 0/4
  loop [===========>                                    ] 1/4
  loop [=======================>                        ] 2/4
  loop [===================================>            ] 3/4
  loop [===============================================>] 4/4
  loop [ <=>                                            ] 5/4
                                                              \r (esc)

test immediate progress completion

  $ hg -y loop 0 2>&1 | $TESTDIR/filtercr.py
  
