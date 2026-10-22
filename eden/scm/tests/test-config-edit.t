#require no-eden

#testcases rust python python-editconfig

#if python
  $ setconfig config.use-rust=false
#endif
#if python-editconfig
  $ setconfig config.use-rust=false experimental.rust-editconfig=false
#endif

hide outer repo
  $ sl init

Edit failure


  $ HGEDITOR=false sl config --edit --quiet
  abort: edit failed: false exited with status 1
  [255]

  $ HGEDITOR=false sl config --user
  opening */sapling/sapling.conf for editing... (glob)
  abort: edit failed: false exited with status 1
  [255]

  $ sl config --user --local
  abort: please specify exactly one config location
  [255]

Invalid arguments

  $ newrepo
  $ sl config --edit missing.value
  abort: invalid config edit: 'missing.value'
  (try section.name=value)
  [255]

  $ sl config --edit missing=name
  abort: invalid config edit: 'missing'
  (try section.name=value)
  [255]

Append configs

  $ sl config --local aa.bb.cc.字 "配
  > 置" ee.fff=gggg
  updated config in $TESTTMP/*/.sl/config (glob)
  $ tail -6 .sl/config | dos2unix
  [aa]
  bb.cc.字 = 配
    置
  
  [ee]
  fff = gggg

Update config in-place without appending

  $ sl config --local aa.bb.cc.字 new_值 "aa.bb.cc.字=新值
  > 测
  > 试
  > "
  updated config in $TESTTMP/*/.sl/config (glob)
  $ tail -7 .sl/config | dos2unix
  [aa]
  bb.cc.字 = 新值
    测
    试
  
  [ee]
  fff = gggg

  $ sl config aa.bb.cc.字
  新值\n测\n试

With comments

  $ newrepo
  $ cat > .sl/config << 'EOF'
  > [a]
  > # b = 1
  > b = 2
  >   second line
  > # b = 3
  > EOF

  $ sl config --local a.b 4
  updated config in $TESTTMP/*/.sl/config (glob)
  $ cat .sl/config
  [a]
  # b = 1
  b = 4
  # b = 3

A missing repo config is created from the sample config

  $ cd
  $ HGIDENTITY=sl newrepo
  $ sl config --local foo.bar baz
  updated config in $TESTTMP/*/.sl/config (glob)
  $ cat .sl/config | dos2unix
  # example repository config (see 'sl help config' for more info)
  [paths]
  # URL aliases to other repo sources
  # (see 'sl help config.paths' for more info)
  #
  # default = https://example.com/example-org/example-repo
  # my-fork = ssh://jdoe@example.com/jdoe/example-repo
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>
  
  [foo]
  bar = baz

User config

  $ sl config --edit a.b=1 --quiet
  $ sl config a.b
  1

  $ sl config --user a.b 2
  updated config in $TESTTMP/*sapling* (glob)
  $ sl config a.b
  2

System config (make sure it tries the right file)

  $ HGEDITOR=false sl config --system
  opening $TESTTMP/config for editing...
  abort: edit failed: false exited with status 1
  [255]
  $ sl config --system sys.key=1
  updated config in $TESTTMP/config
  $ sl config sys.key
  1

Warn about duplicate entries

  $ newrepo
  $ cat > .sl/config << 'EOF'
  > [a]
  > b = 1
  > [a]
  > b = 2
  > EOF

  $ sl config --local a.b=3
  warning: duplicate config entries for a.b in $TESTTMP/*/.sl/config (glob)
  updated config in $TESTTMP/*/.sl/config (glob)
  $ cat .sl/config
  [a]
  b = 1
  [a]
  b = 3
  $ sl config --local -q a.b=4
  $ cat .sl/config
  [a]
  b = 1
  [a]
  b = 4

Can delete in place

  $ newrepo delete
  $ cat > .sl/config << EOF
  > [foo]
  > bar = 1
  > baz = 2
  > EOF

  $ sl config --delete foo.bar
  abort: --delete requires one of --user, --local or --system
  [255]

  $ sl config --delete --local foo.bar=123
  abort: invalid config deletion: 'foo.bar=123'
  (try section.name)
  [255]

  $ sl config --delete --local foo.bar -v
  deleting foo.bar from $TESTTMP/delete/.sl/config
  updated config in $TESTTMP/delete/.sl/config

  $ cat .sl/config
  [foo]
  baz = 2
