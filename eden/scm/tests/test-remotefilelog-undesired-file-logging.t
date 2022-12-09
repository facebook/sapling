#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ cat >> "$TESTTMP/uilog.py" <<EOF
  > from edenscm import extensions
  > from edenscm import ui as uimod
  > def uisetup(ui):
  >   extensions.wrapfunction(uimod.ui, 'log', mylog)
  > def mylog(orig, self, service, *msg, **opts):
  >   if service in ['undesired_file_fetches']:
  >     kw = []
  >     for k, v in sorted(opts.items()):
  >       kw.append("%s=%s" % (k, v))
  >     kwstr = ", ".join(kw)
  >     msgstr = msg[0] % msg[1:]
  >     self.warn('%s: %s (%s)\n' % (service, msgstr, kwstr))
  >   return orig(self, service, *msg, **opts)
  > EOF

  $ cat >> "$HGRCPATH" <<EOF
  > [extensions]
  > uilog=$TESTTMP/uilog.py
  > EOF

  $ newserver master
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ hg commit -qAm x
  $ mkdir dir
  $ echo y > dir/y
  $ hg commit -qAm y
  $ hg push -r tip --to master --create
  pushing rev 79c51fb96423 to destination ssh://user@dummy/master bookmark master
  searching for changes
  exporting bookmark master
  remote: adding changesets (?)
  remote: adding manifests (?)
  remote: adding file changes (?)

  $ cd ..
  $ clone master shallow --noupdate
  $ cd shallow

  $ setconfig scmstore.contentstorefallback=True
  $ hg goto -q master --config remotefilelog.undesiredfileregex=".*" 2>&1 | sort
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06
  fetching tree 'dir' 8a87e5128a9877c501d5a20c32dbd2103a54afad
