#chg-compatible

#require no-fsmonitor

  $ disable treemanifest
  $ hg debugextensions --excludedefault

  $ enable histedit rebase
  $ newext ext1 <<EOF
  > EOF
  $ newext ext2 <<EOF
  > testedwith = '3.0 3.1 3.2.1'
  > buglink = 'https://example.org/bts'
  > EOF

  $ setconfig extensions.hotfix1=python-base64:Cgo=

  $ hg debugextensions --excludedefault
  ext1 (untested!)
  ext2 (3.2.1!)
  histedit
  hotfix1 (untested!)
  rebase

  $ hg debugextensions -v --excludedefault
  ext1
    location: $TESTTMP/ext1.py* (glob)
    bundled: no
  ext2
    location: $TESTTMP/ext2.py* (glob)
    bundled: no
    tested with: 3.0 3.1 3.2.1
    bug reporting: https://example.org/bts
  histedit
    location: */hgext/histedit.py* (glob)
    bundled: yes
  hotfix1
    location: <edenscm_hgext_hotfix1>
    bundled: no
  rebase
    location: */hgext/rebase.py* (glob)
    bundled: yes

  $ hg debugextensions --excludedefault -Tjson | sed 's|\\\\|/|g'
  [
   {
    "buglink": "",
    "bundled": false,
    "name": "ext1",
    "source": "*/ext1.py*", (glob)
    "testedwith": []
   },
   {
    "buglink": "https://example.org/bts",
    "bundled": false,
    "name": "ext2",
    "source": "*/ext2.py*", (glob)
    "testedwith": ["3.0", "3.1", "3.2.1"]
   },
   {
    "buglink": "",
    "bundled": true,
    "name": "histedit",
    "source": "*/hgext/histedit.py*", (glob)
    "testedwith": []
   },
   {
    "buglink": "",
    "bundled": false,
    "name": "hotfix1",
    "source": "<edenscm_hgext_hotfix1>",
    "testedwith": []
   },
   {
    "buglink": "",
    "bundled": true,
    "name": "rebase",
    "source": "*/hgext/rebase.py*", (glob)
    "testedwith": []
   }
  ]

  $ hg debugextensions -T '{ifcontains("3.1", testedwith, "{name}\n")}'
  ext2
  $ hg debugextensions \
  > -T '{ifcontains("3.2", testedwith, "no substring match: {name}\n")}'
