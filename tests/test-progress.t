
  $ cat > loop.py <<EOF
  > from mercurial import commands
  > import time
  > class incrementingtime(object):
  >     def __init__(self):
  >         self._time = 0.0
  >     def __call__(self):
  >         self._time += 0.25
  >         return self._time
  > time.time = incrementingtime()
  > 
  > def loop(ui, loops, **opts):
  >     loops = int(loops)
  >     total = None
  >     if loops >= 0:
  >         total = loops
  >     if opts.get('total', None):
  >         total = int(opts.get('total'))
  >     nested = False
  >     if opts.get('nested', None):
  >         nested = True
  >     loops = abs(loops)
  > 
  >     for i in range(loops):
  >         ui.progress('loop', i, 'loop.%d' % i, 'loopnum', total)
  >         if opts.get('parallel'):
  >             ui.progress('other', i, 'other.%d' % i, 'othernum', total)
  >         if nested:
  >             nested_steps = 2
  >             if i and i % 4 == 0:
  >                 nested_steps = 5
  >             for j in range(nested_steps):
  >                 ui.progress(
  >                   'nested', j, 'nested.%d' % j, 'nestnum', nested_steps)
  >             ui.progress(
  >               'nested', None, 'nested.done', 'nestnum', nested_steps)
  >     ui.progress('loop', None, 'loop.done', 'loopnum', total)
  > 
  > commands.norepo += " loop"
  > 
  > cmdtable = {
  >     "loop": (loop, [('', 'total', '', 'override for total'),
  >                     ('', 'nested', False, 'show nested results'),
  >                     ('', 'parallel', False, 'show parallel sets of results'),
  >                    ],
  >              'hg loop LOOPS'),
  > }
  > EOF

  $ cp $HGRCPATH $HGRCPATH.orig
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "progress=" >> $HGRCPATH
  $ echo "loop=`pwd`/loop.py" >> $HGRCPATH
  $ echo "[progress]" >> $HGRCPATH
  $ echo "format = topic bar number" >> $HGRCPATH
  $ echo "assume-tty=1" >> $HGRCPATH
  $ echo "width=60" >> $HGRCPATH

test default params, display nothing because of delay

  $ hg -y loop 3
  $ echo "delay=0" >> $HGRCPATH
  $ echo "refresh=0" >> $HGRCPATH

test with delay=0, refresh=0

  $ hg -y loop 3
  \r (no-eol) (esc)
  loop [                                                ] 0/3\r (no-eol) (esc)
  loop [===============>                                ] 1/3\r (no-eol) (esc)
  loop [===============================>                ] 2/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)


test nested short-lived topics (which shouldn't display with nestdelay):

  $ hg -y loop 3 --nested
  \r (no-eol) (esc)
  loop [                                                ] 0/3\r (no-eol) (esc)
  loop [===============>                                ] 1/3\r (no-eol) (esc)
  loop [===============================>                ] 2/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

Test nested long-lived topic which has the same name as a short-lived
peer. We shouldn't get stuck showing the short-lived inner steps, and
should go back to skipping the inner steps when the slow nested step
finishes.

  $ hg -y loop 7 --nested
  \r (no-eol) (esc)
  loop [                                                ] 0/7\r (no-eol) (esc)
  loop [=====>                                          ] 1/7\r (no-eol) (esc)
  loop [============>                                   ] 2/7\r (no-eol) (esc)
  loop [===================>                            ] 3/7\r (no-eol) (esc)
  loop [==========================>                     ] 4/7\r (no-eol) (esc)
  nested [==========================>                   ] 3/5\r (no-eol) (esc)
  nested [===================================>          ] 4/5\r (no-eol) (esc)
  loop [=================================>              ] 5/7\r (no-eol) (esc)
  loop [========================================>       ] 6/7\r (no-eol) (esc)
                                                              \r (no-eol) (esc)


  $ hg --config progress.changedelay=0 -y loop 3 --nested
  \r (no-eol) (esc)
  loop [                                                ] 0/3\r (no-eol) (esc)
  nested [                                              ] 0/2\r (no-eol) (esc)
  nested [======================>                       ] 1/2\r (no-eol) (esc)
  loop [===============>                                ] 1/3\r (no-eol) (esc)
  nested [                                              ] 0/2\r (no-eol) (esc)
  nested [======================>                       ] 1/2\r (no-eol) (esc)
  loop [===============================>                ] 2/3\r (no-eol) (esc)
  nested [                                              ] 0/2\r (no-eol) (esc)
  nested [======================>                       ] 1/2\r (no-eol) (esc)
                                                              \r (no-eol) (esc)


test two topics being printed in parallel (as when we're doing a local
--pull clone, where you get the unbundle and bundle progress at the
same time):
  $ hg loop 3 --parallel
  \r (no-eol) (esc)
  loop [                                                ] 0/3\r (no-eol) (esc)
  loop [===============>                                ] 1/3\r (no-eol) (esc)
  loop [===============================>                ] 2/3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
test refresh is taken in account

  $ hg -y --config progress.refresh=100 loop 3

test format options 1

  $ hg -y --config 'progress.format=number topic item+2' loop 2
  \r (no-eol) (esc)
  0/2 loop lo\r (no-eol) (esc)
  1/2 loop lo\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

test format options 2

  $ hg -y --config 'progress.format=number item-3 bar' loop 2
  \r (no-eol) (esc)
  0/2 p.0 [                                                 ]\r (no-eol) (esc)
  1/2 p.1 [=======================>                         ]\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

test format options and indeterminate progress

  $ hg -y --config 'progress.format=number item bar' loop -- -2
  \r (no-eol) (esc)
  0 loop.0               [ <=>                              ]\r (no-eol) (esc)
  1 loop.1               [  <=>                             ]\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

make sure things don't fall over if count > total

  $ hg -y loop --total 4 6
  \r (no-eol) (esc)
  loop [                                                ] 0/4\r (no-eol) (esc)
  loop [===========>                                    ] 1/4\r (no-eol) (esc)
  loop [=======================>                        ] 2/4\r (no-eol) (esc)
  loop [===================================>            ] 3/4\r (no-eol) (esc)
  loop [===============================================>] 4/4\r (no-eol) (esc)
  loop [ <=>                                            ] 5/4\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

test immediate progress completion

  $ hg -y loop 0

test delay time estimates

  $ cat > mocktime.py <<EOF
  > import os
  > import time
  > 
  > class mocktime(object):
  >     def __init__(self, increment):
  >         self.time = 0
  >         self.increment = increment
  >     def __call__(self):
  >         self.time += self.increment
  >         return self.time
  > 
  > def uisetup(ui):
  >     time.time = mocktime(int(os.environ.get('MOCKTIME', '11')))
  > EOF

  $ cp $HGRCPATH.orig $HGRCPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mocktime=`pwd`/mocktime.py" >> $HGRCPATH
  $ echo "progress=" >> $HGRCPATH
  $ echo "loop=`pwd`/loop.py" >> $HGRCPATH
  $ echo "[progress]" >> $HGRCPATH
  $ echo "assume-tty=1" >> $HGRCPATH
  $ echo "delay=25" >> $HGRCPATH
  $ echo "width=60" >> $HGRCPATH

  $ hg -y loop 8
  \r (no-eol) (esc)
  loop [=========>                                ] 2/8 1m07s\r (no-eol) (esc)
  loop [===============>                            ] 3/8 56s\r (no-eol) (esc)
  loop [=====================>                      ] 4/8 45s\r (no-eol) (esc)
  loop [==========================>                 ] 5/8 34s\r (no-eol) (esc)
  loop [================================>           ] 6/8 23s\r (no-eol) (esc)
  loop [=====================================>      ] 7/8 12s\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

  $ MOCKTIME=10000 hg -y loop 4
  \r (no-eol) (esc)
  loop [                                                ] 0/4\r (no-eol) (esc)
  loop [=========>                                ] 1/4 8h21m\r (no-eol) (esc)
  loop [====================>                     ] 2/4 5h34m\r (no-eol) (esc)
  loop [==============================>           ] 3/4 2h47m\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

  $ MOCKTIME=1000000 hg -y loop 4
  \r (no-eol) (esc)
  loop [                                                ] 0/4\r (no-eol) (esc)
  loop [=========>                                ] 1/4 5w00d\r (no-eol) (esc)
  loop [====================>                     ] 2/4 3w03d\r (no-eol) (esc)
  loop [=============================>           ] 3/4 11d14h\r (no-eol) (esc)
                                                              \r (no-eol) (esc)


  $ MOCKTIME=14000000 hg -y loop 4
  \r (no-eol) (esc)
  loop [                                                ] 0/4\r (no-eol) (esc)
  loop [=========>                                ] 1/4 1y18w\r (no-eol) (esc)
  loop [===================>                     ] 2/4 46w03d\r (no-eol) (esc)
  loop [=============================>           ] 3/4 23w02d\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

Time estimates should not fail when there's no end point:
  $ hg -y loop -- -4
  \r (no-eol) (esc)
  loop [ <=>                                              ] 2\r (no-eol) (esc)
  loop [  <=>                                             ] 3\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
