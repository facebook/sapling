  $ hg init t
  $ cd t
  $ echo import > port
  $ hg add port
  $ hg commit -m 0 -u spam -d '0 0'
  $ echo export >> port
  $ hg commit -m 1 -u eggs -d '1 0'
  $ echo export > port
  $ echo vaportight >> port
  $ echo 'import/export' >> port
  $ hg commit -m 2 -u spam -d '2 0'
  $ echo 'import/export' >> port
  $ hg commit -m 3 -u eggs -d '3 0'
  $ head -n 3 port > port1
  $ mv port1 port
  $ hg commit -m 4 -u spam -d '4 0'

pattern error

  $ hg grep '**test**'
  grep: invalid match pattern: nothing to repeat
  [1]

simple

  $ hg grep '.*'
  port:4:export
  port:4:vaportight
  port:4:import/export
  $ hg grep port port
  port:4:export
  port:4:vaportight
  port:4:import/export

simple with color

  $ hg --config extensions.color= grep --config color.mode=ansi \
  >     --color=always port port
  \x1b[0;35mport\x1b[0m\x1b[0;36m:\x1b[0m\x1b[0;32m4\x1b[0m\x1b[0;36m:\x1b[0mex\x1b[0;31;1mport\x1b[0m (esc)
  \x1b[0;35mport\x1b[0m\x1b[0;36m:\x1b[0m\x1b[0;32m4\x1b[0m\x1b[0;36m:\x1b[0mva\x1b[0;31;1mport\x1b[0might (esc)
  \x1b[0;35mport\x1b[0m\x1b[0;36m:\x1b[0m\x1b[0;32m4\x1b[0m\x1b[0;36m:\x1b[0mim\x1b[0;31;1mport\x1b[0m/ex\x1b[0;31;1mport\x1b[0m (esc)

all

  $ hg grep --traceback --all -nu port port
  port:4:4:-:spam:import/export
  port:3:4:+:eggs:import/export
  port:2:1:-:spam:import
  port:2:2:-:spam:export
  port:2:1:+:spam:export
  port:2:2:+:spam:vaportight
  port:2:3:+:spam:import/export
  port:1:2:+:eggs:export
  port:0:1:+:spam:import

other

  $ hg grep -l port port
  port:4
  $ hg grep import port
  port:4:import/export

  $ hg cp port port2
  $ hg commit -m 4 -u spam -d '5 0'

follow

  $ hg grep --traceback -f 'import\n\Z' port2
  port:0:import
  
  $ echo deport >> port2
  $ hg commit -m 5 -u eggs -d '6 0'
  $ hg grep -f --all -nu port port2
  port2:6:4:+:eggs:deport
  port:4:4:-:spam:import/export
  port:3:4:+:eggs:import/export
  port:2:1:-:spam:import
  port:2:2:-:spam:export
  port:2:1:+:spam:export
  port:2:2:+:spam:vaportight
  port:2:3:+:spam:import/export
  port:1:2:+:eggs:export
  port:0:1:+:spam:import

  $ hg up -q null
  $ hg grep -f port
  [1]

  $ cd ..
  $ hg init t2
  $ cd t2
  $ hg grep foobar foo
  [1]
  $ hg grep foobar
  [1]
  $ echo blue >> color
  $ echo black >> color
  $ hg add color
  $ hg ci -m 0
  $ echo orange >> color
  $ hg ci -m 1
  $ echo black > color
  $ hg ci -m 2
  $ echo orange >> color
  $ echo blue >> color
  $ hg ci -m 3
  $ hg grep orange
  color:3:orange
  $ hg grep --all orange
  color:3:+:orange
  color:2:-:orange
  color:1:+:orange


match in last "line" without newline

  $ $PYTHON -c 'fp = open("noeol", "wb"); fp.write("no infinite loop"); fp.close();'
  $ hg ci -Amnoeol
  adding noeol
  $ hg grep loop
  noeol:4:no infinite loop

  $ cd ..

Issue685: traceback in grep -r after rename

Got a traceback when using grep on a single
revision with renamed files.

  $ hg init issue685
  $ cd issue685
  $ echo octarine > color
  $ hg ci -Amcolor
  adding color
  $ hg rename color colour
  $ hg ci -Am rename
  $ hg grep octarine
  colour:1:octarine
  color:0:octarine

Used to crash here

  $ hg grep -r 1 octarine
  colour:1:octarine
  $ cd ..


Issue337: test that grep follows parent-child relationships instead
of just using revision numbers.

  $ hg init issue337
  $ cd issue337

  $ echo white > color
  $ hg commit -A -m "0 white"
  adding color

  $ echo red > color
  $ hg commit -A -m "1 red"

  $ hg update 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo black > color
  $ hg commit -A -m "2 black"
  created new head

  $ hg update --clean 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo blue > color
  $ hg commit -A -m "3 blue"

  $ hg grep --all red
  color:3:-:red
  color:1:+:red

  $ cd ..

  $ hg init a
  $ cd a
  $ cp "$TESTDIR/binfile.bin" .
  $ hg add binfile.bin
  $ hg ci -m 'add binfile.bin'
  $ hg grep "MaCam" --all
  binfile.bin:0:+: Binary file matches

  $ cd ..
