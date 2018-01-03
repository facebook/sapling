  $ PYTHONPATH=$TESTDIR/../:$PYTHONPATH
  $ export PYTHONPATH

  $ cat > loop.py <<EOF
  > from mercurial import commands, registrar
  > import time
  > 
  > from mercurial.extensions import wrapfunction
  > # This borrows heavily from test-progress.t:
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > def uisetup(ui):
  >     def progress(orig, *args, **kwargs):
  >         orig(*args, **kwargs)
  >         if ui.config("progress", "statefile"):
  >             try:
  >                 with open(ui.config("progress", "statefile"), 'r') as f:
  >                     print(f.read())
  >             except IOError as e:
  >                 print(e)
  >     wrapfunction(ui, 'progress', progress)
  > 
  > class incrementingtime(object):
  >     def __init__(self):
  >         self._time = 0.0
  >     def __call__(self):
  >         self._time += 1
  >         return self._time
  > time.time = incrementingtime()
  > 
  > @command('loop',
  >          [('', 'total', '', 'override for total'),
  >           ('', 'nested', False, 'show nested results')],
  >          'hg loop LOOPS',
  >          norepo=True)
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
  >         ui.progress(topiclabel, i, getloopitem(i), 'loopnum', total)
  >         if nested and i % 2 == 0:
  >             nested_steps = 3
  >             for j in range(nested_steps):
  >                 ui.progress(
  >                     'nested', j, 'nested #%d' % j, 'nestnum', nested_steps)
  >             # Sending None completes the progress topic:
  >             ui.progress(
  >                 'nested', None, 'done', 'nestnum', nested_steps)
  >     ui.progress(topiclabel, None, '<last loop iem>', 'loopnum', total)
  > 
  > topiclabel = 'loop'
  > def getloopitem(i):
  >     return 'item #%d' % i
  > EOF
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "progressfile=" >> $HGRCPATH
  $ echo "loop=`pwd`/loop.py" >> $HGRCPATH
  $ echo "[progress]" >> $HGRCPATH
  $ echo "statefile=progress_state" >> $HGRCPATH
  $ echo "assume-tty=1" >> $HGRCPATH
  $ echo "width=60" >> $HGRCPATH
  $ echo "delay=0" >> $HGRCPATH
  $ echo "refresh=0" >> $HGRCPATH
  $ hg -y loop 5
  \r (no-eol) (esc)
  loop [                                                ] 0/5\r (no-eol) (esc)
  loop [=======>                                    ] 1/5 09s\r (no-eol) (esc)
  loop [================>                           ] 2/5 07s\r (no-eol) (esc)
  loop [=========================>                  ] 3/5 05s\r (no-eol) (esc)
  loop [==================================>         ] 4/5 03s\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  {"state": {"loop": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "item #0", "pos": 0, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 5, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {"loop": {"active": true, "estimate_sec": 13, "estimate_str": "13s", "item": "item #1", "pos": 1, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 5, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {"loop": {"active": true, "estimate_sec": 8, "estimate_str": "08s", "item": "item #2", "pos": 2, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 5, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {"loop": {"active": true, "estimate_sec": 5, "estimate_str": "05s", "item": "item #3", "pos": 3, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 5, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {"loop": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "item #4", "pos": 4, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 5, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {}, "topics": []}

  $ hg -y loop --nested 2
  \r (no-eol) (esc)
  loop [                                                ] 0/2\r (no-eol) (esc)
  nested [                                              ] 0/3\r (no-eol) (esc)
  nested [=============>                            ] 1/3 05s\r (no-eol) (esc)
  nested [===========================>              ] 2/3 03s\r (no-eol) (esc)
  loop [=====================>                      ] 1/2 11s\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  {"state": {"loop": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "item #0", "pos": 0, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 2, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {"loop": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "item #0", "pos": 0, "speed_str": null, "topic": "loop", "total": 2, "unit": "loopnum", "units_per_sec": null}, "nested": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "nested #0", "pos": 0, "speed_str": "0 nestnum/sec", "topic": "nested", "total": 3, "unit": "nestnum", "units_per_sec": null}}, "topics": ["loop", "nested"]}
  {"state": {"loop": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "item #0", "pos": 0, "speed_str": null, "topic": "loop", "total": 2, "unit": "loopnum", "units_per_sec": null}, "nested": {"active": true, "estimate_sec": 7, "estimate_str": "07s", "item": "nested #1", "pos": 1, "speed_str": "0 nestnum/sec", "topic": "nested", "total": 3, "unit": "nestnum", "units_per_sec": null}}, "topics": ["loop", "nested"]}
  {"state": {"loop": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "item #0", "pos": 0, "speed_str": null, "topic": "loop", "total": 2, "unit": "loopnum", "units_per_sec": null}, "nested": {"active": true, "estimate_sec": 3, "estimate_str": "03s", "item": "nested #2", "pos": 2, "speed_str": "0 nestnum/sec", "topic": "nested", "total": 3, "unit": "nestnum", "units_per_sec": null}}, "topics": ["loop", "nested"]}
  {"state": {"loop": {"active": false, "estimate_sec": null, "estimate_str": null, "item": "item #0", "pos": 0, "speed_str": null, "topic": "loop", "total": 2, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {"loop": {"active": true, "estimate_sec": 12, "estimate_str": "12s", "item": "item #1", "pos": 1, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 2, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {}, "topics": []}

HGPLAIN=1 hides the ASCII progress bar, but not the progressfile version:
  $ HGPLAIN=1 hg -y loop 2
  {"state": {"loop": {"active": true, "estimate_sec": null, "estimate_str": null, "item": "item #0", "pos": 0, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 2, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {"loop": {"active": true, "estimate_sec": 4, "estimate_str": "04s", "item": "item #1", "pos": 1, "speed_str": "0 loopnum/sec", "topic": "loop", "total": 2, "unit": "loopnum", "units_per_sec": null}}, "topics": ["loop"]}
  {"state": {}, "topics": []}

Do not hide the progress if statefile is not set

  $ hg -y loop 5 --config progress.statefile=
  \r (no-eol) (esc)
  loop [                                                ] 0/5\r (no-eol) (esc)
  loop [=======>                                    ] 1/5 05s\r (no-eol) (esc)
  loop [================>                           ] 2/5 04s\r (no-eol) (esc)
  loop [=========================>                  ] 3/5 03s\r (no-eol) (esc)
  loop [==================================>         ] 4/5 02s\r (no-eol) (esc)
                                                              \r (no-eol) (esc)

