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
    location: */hgext/color.py* (glob)
    tested with: internal
  ext1
    location: */extwithoutinfos.py* (glob)
  histedit
    location: */hgext/histedit.py* (glob)
    tested with: internal
  mq
    location: */hgext/mq.py* (glob)
    tested with: internal
  patchbomb
    location: */hgext/patchbomb.py* (glob)
    tested with: internal
  rebase
    location: */hgext/rebase.py* (glob)
    tested with: internal

  $ hg debugextensions -Tjson | sed 's|\\\\|/|g'
  [
   {
    "buglink": "",
    "name": "color",
    "source": "*/hgext/color.py*", (glob)
    "testedwith": "internal"
   },
   {
    "buglink": "",
    "name": "ext1",
    "source": "*/extwithoutinfos.py*", (glob)
    "testedwith": ""
   },
   {
    "buglink": "",
    "name": "histedit",
    "source": "*/hgext/histedit.py*", (glob)
    "testedwith": "internal"
   },
   {
    "buglink": "",
    "name": "mq",
    "source": "*/hgext/mq.py*", (glob)
    "testedwith": "internal"
   },
   {
    "buglink": "",
    "name": "patchbomb",
    "source": "*/hgext/patchbomb.py*", (glob)
    "testedwith": "internal"
   },
   {
    "buglink": "",
    "name": "rebase",
    "source": "*/hgext/rebase.py*", (glob)
    "testedwith": "internal"
   }
  ]
