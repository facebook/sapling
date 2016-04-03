  $ echo 'raise Exception("bit bucket overflow")' > badext.py
  $ abspathexc=`pwd`/badext.py

  $ cat >baddocext.py <<EOF
  > """
  > baddocext is bad
  > """
  > EOF
  $ abspathdoc=`pwd`/baddocext.py

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > gpg =
  > hgext.gpg =
  > badext = $abspathexc
  > baddocext = $abspathdoc
  > badext2 =
  > EOF

  $ hg -q help help 2>&1 |grep extension
  *** failed to import extension badext from $TESTTMP/badext.py: bit bucket overflow
  *** failed to import extension badext2: No module named badext2

show traceback

  $ hg -q help help --traceback 2>&1 | egrep ' extension|^Exception|Traceback|ImportError'
  *** failed to import extension badext from $TESTTMP/badext.py: bit bucket overflow
  Traceback (most recent call last):
  Exception: bit bucket overflow
  *** failed to import extension badext2: No module named badext2
  Traceback (most recent call last):
  ImportError: No module named badext2

names of extensions failed to load can be accessed via extensions.notloaded()

  $ cat <<EOF > showbadexts.py
  > from mercurial import cmdutil, commands, extensions
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('showbadexts', norepo=True)
  > def showbadexts(ui, *pats, **opts):
  >     ui.write('BADEXTS: %s\n' % ' '.join(sorted(extensions.notloaded())))
  > EOF
  $ hg --config extensions.badexts=showbadexts.py showbadexts 2>&1 | grep '^BADEXTS'
  BADEXTS: badext badext2

show traceback for ImportError of hgext.name if debug is set
(note that --debug option isn't applied yet when loading extensions)

  $ (hg -q help help --traceback --config ui.debug=True 2>&1) \
  > | grep -v '^ ' \
  > | egrep 'extension..[^p]|^Exception|Traceback|ImportError|not import'
  *** failed to import extension badext from $TESTTMP/badext.py: bit bucket overflow
  Traceback (most recent call last):
  Exception: bit bucket overflow
  could not import hgext.badext2 (No module named *badext2): trying badext2 (glob)
  Traceback (most recent call last):
  ImportError: No module named *badext2 (glob)
  could not import hgext3rd.badext2 (No module named *badext2): trying badext2 (glob)
  Traceback (most recent call last):
  ImportError: No module named *badext2 (glob)
  *** failed to import extension badext2: No module named badext2
  Traceback (most recent call last):
  ImportError: No module named badext2

confirm that there's no crash when an extension's documentation is bad

  $ hg help --keyword baddocext
  *** failed to import extension badext from $TESTTMP/badext.py: bit bucket overflow
  *** failed to import extension badext2: No module named badext2
  Topics:
  
   extensions Using Additional Features
