  $ cat >> a.rc << EOF
  > [a]
  > x=1
  > y=2
  > %include b.rc
  > EOF

  $ cat >> b.rc << EOF
  > %include b.rc
  > [b]
  > z = 3
  > [a]
  > %unset y
  > %include broken.rc
  > EOF

  $ cat >> broken.rc << EOF
  > %not-implemented
  > EOF

  >>> from mercurial.rust import config
  >>> cfg = config.config()
  >>> cfg.readpath("a.rc", "readpath", None, None, None)
  >>> cfg.parse("[c]\nx=1", "parse")
  >>> cfg.set("d", "y", "2", "set1")
  >>> cfg.set("d", "x", None, "set2")
  >>> for section in cfg.sections():
  ...     print("section [%s] has names %r" % (section, cfg.names(section)))
  section [a] has names ['x', 'y']
  section [b] has names ['z']
  section [c] has names ['x']
  section [d] has names ['y', 'x']
  >>> print("errors: %r" % cfg.errors())
  errors: ['"$TESTTMP/broken.rc":\n --> 1:2\n  |\n1 | %not-implemented\n  |  ^---\n  |\n  = expected include or unset']
  >>> for item in ["a.x", "a.y", "b.z", "c.x", "d.x", "d.y", "e.x"]:
  ...     section, name = item.split(".")
  ...     print("%s = %r" % (item, cfg.get(section, name)))
  ...     print("  sources: %r" % (cfg.sources(section, name)))
  a.x = '1'
    sources: [('1', ('$TESTTMP/a.rc', 6, 7), 'readpath')]
  a.y = None
    sources: [('2', ('$TESTTMP/a.rc', 10, 11), 'readpath'), (None, ('$TESTTMP/b.rc', 29, 36), 'readpath')]
  b.z = '3'
    sources: [('3', ('$TESTTMP/b.rc', 22, 23), 'readpath')]
  c.x = '1'
    sources: [('1', ('', 6, 7), 'parse')]
  d.x = None
    sources: [(None, None, 'set2')]
  d.y = '2'
    sources: [('2', None, 'set1')]
  e.x = None
    sources: []

Section whitelist

  >>> from mercurial.rust import config
  >>> cfg = config.config()
  >>> cfg.readpath("a.rc", "readpath", ["a"], None, None)
  >>> print(cfg.sections())
  ['a']

Section remap

  >>> from mercurial.rust import config
  >>> cfg = config.config()
  >>> cfg.readpath("a.rc", "readpath", None, {'a': 'x'}.items(), None)
  >>> print(cfg.sections())
  ['x', 'b']

Config whitelist

  >>> from mercurial.rust import config
  >>> cfg = config.config()
  >>> cfg.readpath("a.rc", "readpath", None, None, [('a', 'y')])
  >>> print(cfg.get('a', 'y'))
  None
  >>> print(cfg.get('a', 'x'))
  1

Clone

  >>> from mercurial.rust import config
  >>> cfg1 = config.config()
  >>> cfg1.set("a", "x", "1", "set1")
  >>> cfg2 = cfg1.clone()
  >>> cfg2.set("b", "y", "2", "set2")
  >>> print(cfg2.sections())
  ['a', 'b']
  >>> print(cfg1.sections())
  ['a']
