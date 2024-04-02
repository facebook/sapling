#debugruntest-compatible

#require no-eden


  $ cat > ex1.py <<EOS
  > from sapling import commands, extensions
  > def uisetup(ui):
  >     def files(orig, ui, *args, **kwargs):
  >         ui.status("ex1\n")
  >         return orig(ui, *args, **kwargs)
  >     extensions.wrapcommand(commands.table, "files", files)
  > EOS

  $ cat > ex2.py <<EOS
  > from sapling import commands, extensions
  > def uisetup(ui):
  >     def files(orig, ui, *args, **kwargs):
  >         ui.status("ex2\n")
  >         return orig(ui, *args, **kwargs)
  >     extensions.wrapcommand(commands.table, "files", files)
  > EOS

  $ newrepo
  $ echo foo > foo
  $ hg ci -Aqm foo
  $ hg files
  foo

  $ hg files --config extensions.ex1=~/ex1.py
  ex1
  foo

  $ hg files --config extensions.ex2=~/ex2.py
  ex2
  foo

  $ hg files --config extensions.ex2=~/ex2.py --config extensions.ex1=~/ex1.py
  ex2
  ex1
  foo

  $ hg files
  foo
