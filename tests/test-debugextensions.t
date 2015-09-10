  $ hg debugextensions

  $ debugpath=`pwd`/extwithoutinfos.py

  $ cat > extwithoutinfos.py <<EOF
  > EOF

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > color=
  > histedit=
  > patchbomb=
  > rebase=
  > mq=
  > ext1 = $debugpath
  > EOF

  $ hg debugextensions
  color
  ext1 (untested!)
  histedit
  mq
  patchbomb
  rebase

  $ hg debugextensions -v
  color
    location: */hgext/color.pyc (glob)
    tested with: internal
  ext1
    location: */extwithoutinfos.pyc (glob)
  histedit
    location: */hgext/histedit.pyc (glob)
    tested with: internal
  mq
    location: */hgext/mq.pyc (glob)
    tested with: internal
  patchbomb
    location: */hgext/patchbomb.pyc (glob)
    tested with: internal
  rebase
    location: */hgext/rebase.pyc (glob)
    tested with: internal

  $ hg debugextensions -Tjson
  [
   {
    "buglink": "",
    "name": "color",
    "source": "*/hgext/color.pyc", (glob)
    "testedwith": "internal"
   },
   {
    "buglink": "",
    "name": "ext1",
    "source": "*/extwithoutinfos.pyc", (glob)
    "testedwith": ""
   },
   {
    "buglink": "",
    "name": "histedit",
    "source": "*/hgext/histedit.pyc", (glob)
    "testedwith": "internal"
   },
   {
    "buglink": "",
    "name": "mq",
    "source": "*/hgext/mq.pyc", (glob)
    "testedwith": "internal"
   },
   {
    "buglink": "",
    "name": "patchbomb",
    "source": "*/hgext/patchbomb.pyc", (glob)
    "testedwith": "internal"
   },
   {
    "buglink": "",
    "name": "rebase",
    "source": "*/hgext/rebase.pyc", (glob)
    "testedwith": "internal"
   }
  ]
