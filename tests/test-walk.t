  $ hg init t
  $ cd t
  $ mkdir -p beans
  $ for b in kidney navy turtle borlotti black pinto; do
  >     echo $b > beans/$b
  > done
  $ mkdir -p mammals/Procyonidae
  $ for m in cacomistle coatimundi raccoon; do
  >     echo $m > mammals/Procyonidae/$m
  > done
  $ echo skunk > mammals/skunk
  $ echo fennel > fennel
  $ echo fenugreek > fenugreek
  $ echo fiddlehead > fiddlehead
  $ hg addremove
  adding beans/black
  adding beans/borlotti
  adding beans/kidney
  adding beans/navy
  adding beans/pinto
  adding beans/turtle
  adding fennel
  adding fenugreek
  adding fiddlehead
  adding mammals/Procyonidae/cacomistle
  adding mammals/Procyonidae/coatimundi
  adding mammals/Procyonidae/raccoon
  adding mammals/skunk
  $ hg commit -m "commit #0"

  $ hg debugwalk
  matcher: <alwaysmatcher>
  f  beans/black                     beans/black
  f  beans/borlotti                  beans/borlotti
  f  beans/kidney                    beans/kidney
  f  beans/navy                      beans/navy
  f  beans/pinto                     beans/pinto
  f  beans/turtle                    beans/turtle
  f  fennel                          fennel
  f  fenugreek                       fenugreek
  f  fiddlehead                      fiddlehead
  f  mammals/Procyonidae/cacomistle  mammals/Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  mammals/Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     mammals/Procyonidae/raccoon
  f  mammals/skunk                   mammals/skunk
  $ hg debugwalk -I.
  matcher: <includematcher includes='(?:)'>
  f  beans/black                     beans/black
  f  beans/borlotti                  beans/borlotti
  f  beans/kidney                    beans/kidney
  f  beans/navy                      beans/navy
  f  beans/pinto                     beans/pinto
  f  beans/turtle                    beans/turtle
  f  fennel                          fennel
  f  fenugreek                       fenugreek
  f  fiddlehead                      fiddlehead
  f  mammals/Procyonidae/cacomistle  mammals/Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  mammals/Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     mammals/Procyonidae/raccoon
  f  mammals/skunk                   mammals/skunk

  $ cd mammals
  $ hg debugwalk
  matcher: <alwaysmatcher>
  f  beans/black                     ../beans/black
  f  beans/borlotti                  ../beans/borlotti
  f  beans/kidney                    ../beans/kidney
  f  beans/navy                      ../beans/navy
  f  beans/pinto                     ../beans/pinto
  f  beans/turtle                    ../beans/turtle
  f  fennel                          ../fennel
  f  fenugreek                       ../fenugreek
  f  fiddlehead                      ../fiddlehead
  f  mammals/Procyonidae/cacomistle  Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     Procyonidae/raccoon
  f  mammals/skunk                   skunk
  $ hg debugwalk -X ../beans
  matcher: <differencematcher m1=<alwaysmatcher>, m2=<includematcher includes='(?:beans(?:/|$))'>>
  f  fennel                          ../fennel
  f  fenugreek                       ../fenugreek
  f  fiddlehead                      ../fiddlehead
  f  mammals/Procyonidae/cacomistle  Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     Procyonidae/raccoon
  f  mammals/skunk                   skunk
  $ hg debugwalk -I '*k'
  matcher: <includematcher includes='(?:mammals\\/[^/]*k(?:/|$))'>
  f  mammals/skunk  skunk
  $ hg debugwalk -I 'glob:*k'
  matcher: <includematcher includes='(?:mammals\\/[^/]*k(?:/|$))'>
  f  mammals/skunk  skunk
  $ hg debugwalk -I 'relglob:*k'
  matcher: <includematcher includes='(?:(?:|.*/)[^/]*k(?:/|$))'>
  f  beans/black    ../beans/black
  f  fenugreek      ../fenugreek
  f  mammals/skunk  skunk
  $ hg debugwalk -I 'relglob:*k' .
  matcher: <intersectionmatcher m1=<patternmatcher patterns='(?:mammals(?:/|$))'>, m2=<includematcher includes='(?:(?:|.*/)[^/]*k(?:/|$))'>>
  f  mammals/skunk  skunk
  $ hg debugwalk -I 're:.*k$'
  matcher: <includematcher includes='(?:.*k$)'>
  f  beans/black    ../beans/black
  f  fenugreek      ../fenugreek
  f  mammals/skunk  skunk
  $ hg debugwalk -I 'relre:.*k$'
  matcher: <includematcher includes='(?:.*.*k$)'>
  f  beans/black    ../beans/black
  f  fenugreek      ../fenugreek
  f  mammals/skunk  skunk
  $ hg debugwalk -I 'path:beans'
  matcher: <includematcher includes='(?:beans(?:/|$))'>
  f  beans/black     ../beans/black
  f  beans/borlotti  ../beans/borlotti
  f  beans/kidney    ../beans/kidney
  f  beans/navy      ../beans/navy
  f  beans/pinto     ../beans/pinto
  f  beans/turtle    ../beans/turtle
  $ hg debugwalk -I 'relpath:detour/../../beans'
  matcher: <includematcher includes='(?:beans(?:/|$))'>
  f  beans/black     ../beans/black
  f  beans/borlotti  ../beans/borlotti
  f  beans/kidney    ../beans/kidney
  f  beans/navy      ../beans/navy
  f  beans/pinto     ../beans/pinto
  f  beans/turtle    ../beans/turtle

  $ hg debugwalk 'rootfilesin:'
  matcher: <patternmatcher patterns='(?:[^/]+$)'>
  f  fennel      ../fennel
  f  fenugreek   ../fenugreek
  f  fiddlehead  ../fiddlehead
  $ hg debugwalk -I 'rootfilesin:'
  matcher: <includematcher includes='(?:[^/]+$)'>
  f  fennel      ../fennel
  f  fenugreek   ../fenugreek
  f  fiddlehead  ../fiddlehead
  $ hg debugwalk 'rootfilesin:.'
  matcher: <patternmatcher patterns='(?:[^/]+$)'>
  f  fennel      ../fennel
  f  fenugreek   ../fenugreek
  f  fiddlehead  ../fiddlehead
  $ hg debugwalk -I 'rootfilesin:.'
  matcher: <includematcher includes='(?:[^/]+$)'>
  f  fennel      ../fennel
  f  fenugreek   ../fenugreek
  f  fiddlehead  ../fiddlehead
  $ hg debugwalk -X 'rootfilesin:'
  matcher: <differencematcher m1=<alwaysmatcher>, m2=<includematcher includes='(?:[^/]+$)'>>
  f  beans/black                     ../beans/black
  f  beans/borlotti                  ../beans/borlotti
  f  beans/kidney                    ../beans/kidney
  f  beans/navy                      ../beans/navy
  f  beans/pinto                     ../beans/pinto
  f  beans/turtle                    ../beans/turtle
  f  mammals/Procyonidae/cacomistle  Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     Procyonidae/raccoon
  f  mammals/skunk                   skunk
  $ hg debugwalk 'rootfilesin:fennel'
  matcher: <patternmatcher patterns='(?:fennel/[^/]+$)'>
  $ hg debugwalk -I 'rootfilesin:fennel'
  matcher: <includematcher includes='(?:fennel/[^/]+$)'>
  $ hg debugwalk 'rootfilesin:skunk'
  matcher: <patternmatcher patterns='(?:skunk/[^/]+$)'>
  $ hg debugwalk -I 'rootfilesin:skunk'
  matcher: <includematcher includes='(?:skunk/[^/]+$)'>
  $ hg debugwalk 'rootfilesin:beans'
  matcher: <patternmatcher patterns='(?:beans/[^/]+$)'>
  f  beans/black     ../beans/black
  f  beans/borlotti  ../beans/borlotti
  f  beans/kidney    ../beans/kidney
  f  beans/navy      ../beans/navy
  f  beans/pinto     ../beans/pinto
  f  beans/turtle    ../beans/turtle
  $ hg debugwalk -I 'rootfilesin:beans'
  matcher: <includematcher includes='(?:beans/[^/]+$)'>
  f  beans/black     ../beans/black
  f  beans/borlotti  ../beans/borlotti
  f  beans/kidney    ../beans/kidney
  f  beans/navy      ../beans/navy
  f  beans/pinto     ../beans/pinto
  f  beans/turtle    ../beans/turtle
  $ hg debugwalk 'rootfilesin:mammals'
  matcher: <patternmatcher patterns='(?:mammals/[^/]+$)'>
  f  mammals/skunk  skunk
  $ hg debugwalk -I 'rootfilesin:mammals'
  matcher: <includematcher includes='(?:mammals/[^/]+$)'>
  f  mammals/skunk  skunk
  $ hg debugwalk 'rootfilesin:mammals/'
  matcher: <patternmatcher patterns='(?:mammals/[^/]+$)'>
  f  mammals/skunk  skunk
  $ hg debugwalk -I 'rootfilesin:mammals/'
  matcher: <includematcher includes='(?:mammals/[^/]+$)'>
  f  mammals/skunk  skunk
  $ hg debugwalk -X 'rootfilesin:mammals'
  matcher: <differencematcher m1=<alwaysmatcher>, m2=<includematcher includes='(?:mammals/[^/]+$)'>>
  f  beans/black                     ../beans/black
  f  beans/borlotti                  ../beans/borlotti
  f  beans/kidney                    ../beans/kidney
  f  beans/navy                      ../beans/navy
  f  beans/pinto                     ../beans/pinto
  f  beans/turtle                    ../beans/turtle
  f  fennel                          ../fennel
  f  fenugreek                       ../fenugreek
  f  fiddlehead                      ../fiddlehead
  f  mammals/Procyonidae/cacomistle  Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     Procyonidae/raccoon

  $ hg debugwalk .
  matcher: <patternmatcher patterns='(?:mammals(?:/|$))'>
  f  mammals/Procyonidae/cacomistle  Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     Procyonidae/raccoon
  f  mammals/skunk                   skunk
  $ hg debugwalk -I.
  matcher: <includematcher includes='(?:mammals(?:/|$))'>
  f  mammals/Procyonidae/cacomistle  Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     Procyonidae/raccoon
  f  mammals/skunk                   skunk
  $ hg debugwalk Procyonidae
  matcher: <patternmatcher patterns='(?:mammals\\/Procyonidae(?:/|$))'>
  f  mammals/Procyonidae/cacomistle  Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     Procyonidae/raccoon

  $ cd Procyonidae
  $ hg debugwalk .
  matcher: <patternmatcher patterns='(?:mammals\\/Procyonidae(?:/|$))'>
  f  mammals/Procyonidae/cacomistle  cacomistle
  f  mammals/Procyonidae/coatimundi  coatimundi
  f  mammals/Procyonidae/raccoon     raccoon
  $ hg debugwalk ..
  matcher: <patternmatcher patterns='(?:mammals(?:/|$))'>
  f  mammals/Procyonidae/cacomistle  cacomistle
  f  mammals/Procyonidae/coatimundi  coatimundi
  f  mammals/Procyonidae/raccoon     raccoon
  f  mammals/skunk                   ../skunk
  $ cd ..

  $ hg debugwalk ../beans
  matcher: <patternmatcher patterns='(?:beans(?:/|$))'>
  f  beans/black     ../beans/black
  f  beans/borlotti  ../beans/borlotti
  f  beans/kidney    ../beans/kidney
  f  beans/navy      ../beans/navy
  f  beans/pinto     ../beans/pinto
  f  beans/turtle    ../beans/turtle
  $ hg debugwalk .
  matcher: <patternmatcher patterns='(?:mammals(?:/|$))'>
  f  mammals/Procyonidae/cacomistle  Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     Procyonidae/raccoon
  f  mammals/skunk                   skunk
  $ hg debugwalk .hg
  abort: path 'mammals/.hg' is inside nested repo 'mammals'
  [255]
  $ hg debugwalk ../.hg
  abort: path contains illegal component: .hg
  [255]
  $ cd ..

  $ hg debugwalk -Ibeans
  matcher: <includematcher includes='(?:beans(?:/|$))'>
  f  beans/black     beans/black
  f  beans/borlotti  beans/borlotti
  f  beans/kidney    beans/kidney
  f  beans/navy      beans/navy
  f  beans/pinto     beans/pinto
  f  beans/turtle    beans/turtle
  $ hg debugwalk -I '{*,{b,m}*/*}k'
  matcher: <includematcher includes='(?:(?:[^/]*|(?:b|m)[^/]*\\/[^/]*)k(?:/|$))'>
  f  beans/black    beans/black
  f  fenugreek      fenugreek
  f  mammals/skunk  mammals/skunk
  $ hg debugwalk -Ibeans mammals
  matcher: <intersectionmatcher m1=<patternmatcher patterns='(?:mammals(?:/|$))'>, m2=<includematcher includes='(?:beans(?:/|$))'>>
  $ hg debugwalk -Inon-existent
  matcher: <includematcher includes='(?:non\\-existent(?:/|$))'>
  $ hg debugwalk -Inon-existent -Ibeans/black
  matcher: <includematcher includes='(?:non\\-existent(?:/|$)|beans\\/black(?:/|$))'>
  f  beans/black  beans/black
  $ hg debugwalk -Ibeans beans/black
  matcher: <intersectionmatcher m1=<patternmatcher patterns='(?:beans\\/black(?:/|$))'>, m2=<includematcher includes='(?:beans(?:/|$))'>>
  f  beans/black  beans/black  exact
  $ hg debugwalk -Ibeans/black beans
  matcher: <intersectionmatcher m1=<patternmatcher patterns='(?:beans(?:/|$))'>, m2=<includematcher includes='(?:beans\\/black(?:/|$))'>>
  f  beans/black  beans/black
  $ hg debugwalk -Xbeans/black beans
  matcher: <differencematcher m1=<patternmatcher patterns='(?:beans(?:/|$))'>, m2=<includematcher includes='(?:beans\\/black(?:/|$))'>>
  f  beans/borlotti  beans/borlotti
  f  beans/kidney    beans/kidney
  f  beans/navy      beans/navy
  f  beans/pinto     beans/pinto
  f  beans/turtle    beans/turtle
  $ hg debugwalk -Xbeans/black -Ibeans
  matcher: <differencematcher m1=<includematcher includes='(?:beans(?:/|$))'>, m2=<includematcher includes='(?:beans\\/black(?:/|$))'>>
  f  beans/borlotti  beans/borlotti
  f  beans/kidney    beans/kidney
  f  beans/navy      beans/navy
  f  beans/pinto     beans/pinto
  f  beans/turtle    beans/turtle
  $ hg debugwalk -Xbeans/black beans/black
  matcher: <differencematcher m1=<patternmatcher patterns='(?:beans\\/black(?:/|$))'>, m2=<includematcher includes='(?:beans\\/black(?:/|$))'>>
  f  beans/black  beans/black  exact
  $ hg debugwalk -Xbeans/black -Ibeans/black
  matcher: <differencematcher m1=<includematcher includes='(?:beans\\/black(?:/|$))'>, m2=<includematcher includes='(?:beans\\/black(?:/|$))'>>
  $ hg debugwalk -Xbeans beans/black
  matcher: <differencematcher m1=<patternmatcher patterns='(?:beans\\/black(?:/|$))'>, m2=<includematcher includes='(?:beans(?:/|$))'>>
  f  beans/black  beans/black  exact
  $ hg debugwalk -Xbeans -Ibeans/black
  matcher: <differencematcher m1=<includematcher includes='(?:beans\\/black(?:/|$))'>, m2=<includematcher includes='(?:beans(?:/|$))'>>
  $ hg debugwalk 'glob:mammals/../beans/b*'
  matcher: <patternmatcher patterns='(?:beans\\/b[^/]*$)'>
  f  beans/black     beans/black
  f  beans/borlotti  beans/borlotti
  $ hg debugwalk '-X*/Procyonidae' mammals
  matcher: <differencematcher m1=<patternmatcher patterns='(?:mammals(?:/|$))'>, m2=<includematcher includes='(?:[^/]*\\/Procyonidae(?:/|$))'>>
  f  mammals/skunk  mammals/skunk
  $ hg debugwalk path:mammals
  matcher: <patternmatcher patterns='(?:mammals(?:/|$))'>
  f  mammals/Procyonidae/cacomistle  mammals/Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  mammals/Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     mammals/Procyonidae/raccoon
  f  mammals/skunk                   mammals/skunk
  $ hg debugwalk ..
  abort: .. not under root '$TESTTMP/t'
  [255]
  $ hg debugwalk beans/../..
  abort: beans/../.. not under root '$TESTTMP/t'
  [255]
  $ hg debugwalk .hg
  abort: path contains illegal component: .hg
  [255]
  $ hg debugwalk beans/../.hg
  abort: path contains illegal component: .hg
  [255]
  $ hg debugwalk beans/../.hg/data
  abort: path contains illegal component: .hg/data
  [255]
  $ hg debugwalk beans/.hg
  abort: path 'beans/.hg' is inside nested repo 'beans'
  [255]

Test absolute paths:

  $ hg debugwalk `pwd`/beans
  matcher: <patternmatcher patterns='(?:beans(?:/|$))'>
  f  beans/black     beans/black
  f  beans/borlotti  beans/borlotti
  f  beans/kidney    beans/kidney
  f  beans/navy      beans/navy
  f  beans/pinto     beans/pinto
  f  beans/turtle    beans/turtle
  $ hg debugwalk `pwd`/..
  abort: $TESTTMP/t/.. not under root '$TESTTMP/t'
  [255]

Test patterns:

  $ hg debugwalk glob:\*
  matcher: <patternmatcher patterns='(?:[^/]*$)'>
  f  fennel      fennel
  f  fenugreek   fenugreek
  f  fiddlehead  fiddlehead
#if eol-in-paths
  $ echo glob:glob > glob:glob
  $ hg addremove
  adding glob:glob
  warning: filename contains ':', which is reserved on Windows: 'glob:glob'
  $ hg debugwalk glob:\*
  matcher: <patternmatcher patterns='(?:[^/]*$)'>
  f  fennel      fennel
  f  fenugreek   fenugreek
  f  fiddlehead  fiddlehead
  f  glob:glob   glob:glob
  $ hg debugwalk glob:glob
  matcher: <patternmatcher patterns='(?:glob$)'>
  glob: $ENOENT$
  $ hg debugwalk glob:glob:glob
  matcher: <patternmatcher patterns='(?:glob\\:glob$)'>
  f  glob:glob  glob:glob  exact
  $ hg debugwalk path:glob:glob
  matcher: <patternmatcher patterns='(?:glob\\:glob(?:/|$))'>
  f  glob:glob  glob:glob  exact
  $ rm glob:glob
  $ hg addremove
  removing glob:glob
#endif

  $ hg debugwalk 'glob:**e'
  matcher: <patternmatcher patterns='(?:.*e$)'>
  f  beans/turtle                    beans/turtle
  f  mammals/Procyonidae/cacomistle  mammals/Procyonidae/cacomistle

  $ hg debugwalk 're:.*[kb]$'
  matcher: <patternmatcher patterns='(?:.*[kb]$)'>
  f  beans/black    beans/black
  f  fenugreek      fenugreek
  f  mammals/skunk  mammals/skunk

  $ hg debugwalk path:beans/black
  matcher: <patternmatcher patterns='(?:beans\\/black(?:/|$))'>
  f  beans/black  beans/black  exact
  $ hg debugwalk path:beans//black
  matcher: <patternmatcher patterns='(?:beans\\/black(?:/|$))'>
  f  beans/black  beans/black  exact

  $ hg debugwalk relglob:Procyonidae
  matcher: <patternmatcher patterns='(?:(?:|.*/)Procyonidae$)'>
  $ hg debugwalk 'relglob:Procyonidae/**'
  matcher: <patternmatcher patterns='(?:(?:|.*/)Procyonidae\\/.*$)'>
  f  mammals/Procyonidae/cacomistle  mammals/Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  mammals/Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     mammals/Procyonidae/raccoon
  $ hg debugwalk 'relglob:Procyonidae/**' fennel
  matcher: <patternmatcher patterns='(?:(?:|.*/)Procyonidae\\/.*$|fennel(?:/|$))'>
  f  fennel                          fennel                          exact
  f  mammals/Procyonidae/cacomistle  mammals/Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  mammals/Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     mammals/Procyonidae/raccoon
  $ hg debugwalk beans 'glob:beans/*'
  matcher: <patternmatcher patterns='(?:beans(?:/|$)|beans\\/[^/]*$)'>
  f  beans/black     beans/black
  f  beans/borlotti  beans/borlotti
  f  beans/kidney    beans/kidney
  f  beans/navy      beans/navy
  f  beans/pinto     beans/pinto
  f  beans/turtle    beans/turtle
  $ hg debugwalk 'glob:mamm**'
  matcher: <patternmatcher patterns='(?:mamm.*$)'>
  f  mammals/Procyonidae/cacomistle  mammals/Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  mammals/Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     mammals/Procyonidae/raccoon
  f  mammals/skunk                   mammals/skunk
  $ hg debugwalk 'glob:mamm**' fennel
  matcher: <patternmatcher patterns='(?:mamm.*$|fennel(?:/|$))'>
  f  fennel                          fennel                          exact
  f  mammals/Procyonidae/cacomistle  mammals/Procyonidae/cacomistle
  f  mammals/Procyonidae/coatimundi  mammals/Procyonidae/coatimundi
  f  mammals/Procyonidae/raccoon     mammals/Procyonidae/raccoon
  f  mammals/skunk                   mammals/skunk
  $ hg debugwalk 'glob:j*'
  matcher: <patternmatcher patterns='(?:j[^/]*$)'>
  $ hg debugwalk NOEXIST
  matcher: <patternmatcher patterns='(?:NOEXIST(?:/|$))'>
  NOEXIST: * (glob)

#if fifo
  $ mkfifo fifo
  $ hg debugwalk fifo
  matcher: <patternmatcher patterns='(?:fifo(?:/|$))'>
  fifo: unsupported file type (type is fifo) (no-fsmonitor !)
#endif

  $ rm fenugreek
  $ hg debugwalk fenugreek
  matcher: <patternmatcher patterns='(?:fenugreek(?:/|$))'>
  f  fenugreek  fenugreek  exact
  $ hg rm fenugreek
  $ hg debugwalk fenugreek
  matcher: <patternmatcher patterns='(?:fenugreek(?:/|$))'>
  f  fenugreek  fenugreek  exact
  $ touch new
  $ hg debugwalk new
  matcher: <patternmatcher patterns='(?:new(?:/|$))'>
  f  new  new  exact

  $ mkdir ignored
  $ touch ignored/file
  $ echo 'ignored' > .gitignore
  $ hg debugwalk ignored
  matcher: <patternmatcher patterns='(?:ignored(?:/|$))'>
  $ hg debugwalk ignored/file
  matcher: <patternmatcher patterns='(?:ignored\\/file(?:/|$))'>
  f  ignored/file  ignored/file  exact

Test listfile and listfile0

  $ $PYTHON -c "file('listfile0', 'wb').write('fenugreek\0new\0')"
  $ hg debugwalk -I 'listfile0:listfile0'
  matcher: <includematcher includes='(?:fenugreek(?:/|$)|new(?:/|$))'>
  f  fenugreek  fenugreek
  f  new        new
  $ $PYTHON -c "file('listfile', 'wb').write('fenugreek\nnew\r\nmammals/skunk\n')"
  $ hg debugwalk -I 'listfile:listfile'
  matcher: <includematcher includes='(?:fenugreek(?:/|$)|new(?:/|$)|mammals\\/skunk(?:/|$))'>
  f  fenugreek      fenugreek
  f  mammals/skunk  mammals/skunk
  f  new            new

  $ cd ..
  $ hg debugwalk -R t t/mammals/skunk
  matcher: <patternmatcher patterns='(?:mammals\\/skunk(?:/|$))'>
  f  mammals/skunk  t/mammals/skunk  exact
  $ mkdir t2
  $ cd t2
  $ hg debugwalk -R ../t ../t/mammals/skunk
  matcher: <patternmatcher patterns='(?:mammals\\/skunk(?:/|$))'>
  f  mammals/skunk  ../t/mammals/skunk  exact
  $ hg debugwalk --cwd ../t mammals/skunk
  matcher: <patternmatcher patterns='(?:mammals\\/skunk(?:/|$))'>
  f  mammals/skunk  mammals/skunk  exact

  $ cd ..

Test split patterns on overflow

  $ cd t
  $ echo fennel > overflow.list
  $ $PYTHON -c "for i in xrange(20000 / 100): print 'x' * 100" >> overflow.list
  $ echo fenugreek >> overflow.list
  $ hg debugwalk 'listfile:overflow.list' 2>&1 | egrep -v '(^matcher: |^xxx)'
  f  fennel     fennel     exact
  f  fenugreek  fenugreek  exact
  $ cd ..
