#require no-windows no-eden
#inprocess-hg-incompatible

  $ enable morestatus
  $ setconfig morestatus.show=true
  $ setconfig rebase.experimental.inmemory=true

  $ setconfig experimental.mergedriver=python:$TESTTMP/mergedriver-test.py
  $ cat > $TESTTMP/mergedriver-test.py << EOF
  > import time
  > import shutil
  > 
  > def preprocess(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     unresolved_files = list(mergestate.unresolved())
  > 
  >     for unresolved_file in unresolved_files:
  >         if unresolved_file in ("bar", "baz"):
  >             mergestate.mark(unresolved_file, 'd')
  > 
  >     mergestate.commit()
  > 
  > def conclude(ui, repo, hooktype, mergestate, wctx, labels=None):
  >     unresolvedfiles = list(mergestate.driverresolved())
  >     for f in unresolvedfiles:
  >         shutil.copyfile("foo", f)
  >         mergestate.mark(f, "r")
  >     if "baz" in unresolvedfiles:
  >         ui.status(f"  conclude sleeping ...\n")
  >         time.sleep(5)
  >     ui.status("  conclude done\n")
  > EOF

  $ cat > $TESTTMP/myrebase.py << EOF
  > import os
  > 
  > with open("pid", "w") as f:
  >     f.write("%s\n" % os.getpid())
  > 
  > opts = {"source": "fbc6d9483227", "dest": "92cc5e5f07f6"}
  > sapling.ext.rebase.rebase(ui, repo, templ=None, **opts)
  > EOF

  $ cat > $TESTTMP/interrupt.py << EOF
  > import time, os, signal, sys
  > for tick in range(1000):
  >     if not os.path.exists("pid") or not open("pid").read():
  >         time.sleep(1)
  > time.sleep(1)
  > pid = int(open("pid").read())
  > os.kill(pid, signal.SIGINT)
  > try:
  >     for _ in range(30):
  >         os.kill(pid, 0)  # raise if pid no loner exists
  >         time.sleep(1)
  > except Exception:
  >     pass
  > EOF

Test merge driver is invoked only once

  $ newclientrepo
  $ drawdag <<'EOS'
  >   E  # E/foo = 1\n2\n3b\n4\n
  >   |
  >   D  # D/baz= 1b\n2\n3\n
  >   | 
  > B C  # C/foo = 1\n2\n3b\n
  > |/   # B/foo = 1a\n2\n3\n
  > A    # B/bar = 1a\n2\n3\n
  >      # B/baz = 1a\n2\n3\n
  >      # A/foo = 1\n2\n3\n
  >      # A/bar = 1\n2\n3\n
  >      # A/baz = 1\n2\n3\n
  >      # drawdag.defaultfiles=false
  > EOS
  $ hg log -G -T "{node|short} {desc}"
  o  9acff5452ac4 E
  │
  o  5936dc4cac62 D
  │
  o  fbc6d9483227 C
  │
  │ o  92cc5e5f07f6 B
  ├─╯
  o  2cacf0e4c790 A

  $ hg up -q $E
  $ ( hg dbsh $TESTTMP/myrebase.py; echo $?; ) >out 2>err &

  $ hg debugpython -- $TESTTMP/interrupt.py
  $ hg st
  M baz
  ? err
  ? out
  ? pid
  
  # The repository is in an unfinished *rebase* state.
  # No unresolved merge conflicts.
  # To continue:                hg rebase --continue
  # To abort:                   hg rebase --abort
  # To quit:                    hg rebase --quit
  $ cat out
  rebasing fbc6d9483227 "C"
  merging foo
  rebasing 5936dc4cac62 "D"
  rebasing 5936dc4cac62 "D"
    conclude sleeping ...
  130
  $ hg log -G -T "{node|short} {desc}"
  @  cd9c7a0f594d C
  │
  │ o  9acff5452ac4 E
  │ │
  │ o  5936dc4cac62 D
  │ │
  │ x  fbc6d9483227 C
  │ │
  o │  92cc5e5f07f6 B
  ├─╯
  o  2cacf0e4c790 A

Test merge driver is invoked multiple times

  $ newclientrepo
  $ drawdag <<'EOS'
  >   F  # F/baz = 1c\n2\n3\n
  >   |
  >   E  # E/foo = 1\n2\n3b\n4\n
  >   |
  >   D  # D/bar = 1b\n2\n3\n
  >   | 
  > B C  # C/foo = 1\n2\n3b\n
  > |/   # B/foo = 1a\n2\n3\n
  > A    # B/bar = 1a\n2\n3\n
  >      # B/baz = 1a\n2\n3\n
  >      # A/foo = 1\n2\n3\n
  >      # A/bar = 1\n2\n3\n
  >      # A/baz = 1\n2\n3\n
  >      # drawdag.defaultfiles=false
  > EOS
  $ hg log -G -T "{node|short} {desc}"
  o  e29b137ee2c0 F
  │
  o  8ec7c4b0f139 E
  │
  o  3221ce790155 D
  │
  o  fbc6d9483227 C
  │
  │ o  92cc5e5f07f6 B
  ├─╯
  o  2cacf0e4c790 A

  $ hg up -q $F
  $ ( hg dbsh $TESTTMP/myrebase.py; echo $?; ) >out 2>err &

  $ hg debugpython -- $TESTTMP/interrupt.py
  $ hg st
  M baz
  ? err
  ? out
  ? pid
  
  # The repository is in an unfinished *rebase* state.
  # No unresolved merge conflicts.
  # To continue:                hg rebase --continue
  # To abort:                   hg rebase --abort
  # To quit:                    hg rebase --quit
  $ cat out
  rebasing fbc6d9483227 "C"
  merging foo
  rebasing 3221ce790155 "D"
  rebasing 3221ce790155 "D"
    conclude done
  rebasing 8ec7c4b0f139 "E"
  merging foo
  rebasing e29b137ee2c0 "F"
    conclude sleeping ...
  130
  $ hg log -G -T "{node|short} {desc}"
  @  15fd84eef4a1 E
  │
  o  7da75ead7207 D
  │
  o  cd9c7a0f594d C
  │
  │ o  e29b137ee2c0 F
  │ │
  │ x  8ec7c4b0f139 E
  │ │
  │ x  3221ce790155 D
  │ │
  │ x  fbc6d9483227 C
  │ │
  o │  92cc5e5f07f6 B
  ├─╯
  o  2cacf0e4c790 A
