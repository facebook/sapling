  $ hg debugextensions

  $ debugpath=`pwd`/extwithoutinfos.py

  $ cat > extwithoutinfos.py <<EOF
  > EOF
  $ cat > extwithinfos.py <<EOF
  > testedwith = '3.0 3.1 3.2.1'
  > buglink = 'https://example.org/bts'
  > EOF

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > color=
  > histedit=
  > patchbomb=
  > rebase=
  > mq=
  > ext1 = $debugpath
  > ext2 = `pwd`/extwithinfos.py
  > EOF

  $ hg debugextensions
  color
  ext1 (untested!)
  ext2 (3.2.1!)
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
  ext2
    location: */extwithinfos.py* (glob)
    tested with: 3.0 3.1 3.2.1
    bug reporting: https://example.org/bts
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
    "testedwith": ["internal"]
   },
   {
    "buglink": "",
    "name": "ext1",
    "source": "*/extwithoutinfos.py*", (glob)
    "testedwith": []
   },
   {
    "buglink": "https://example.org/bts",
    "name": "ext2",
    "source": "*/extwithinfos.py*", (glob)
    "testedwith": ["3.0", "3.1", "3.2.1"]
   },
   {
    "buglink": "",
    "name": "histedit",
    "source": "*/hgext/histedit.py*", (glob)
    "testedwith": ["internal"]
   },
   {
    "buglink": "",
    "name": "mq",
    "source": "*/hgext/mq.py*", (glob)
    "testedwith": ["internal"]
   },
   {
    "buglink": "",
    "name": "patchbomb",
    "source": "*/hgext/patchbomb.py*", (glob)
    "testedwith": ["internal"]
   },
   {
    "buglink": "",
    "name": "rebase",
    "source": "*/hgext/rebase.py*", (glob)
    "testedwith": ["internal"]
   }
  ]

  $ hg debugextensions -T '{ifcontains("3.1", testedwith, "{name}\n")}'
  ext2
  $ hg debugextensions \
  > -T '{ifcontains("3.2", testedwith, "no substring match: {name}\n")}'
