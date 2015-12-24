  $ echo 'raise Exception("bit bucket overflow")' > badext.py
  $ abspath=`pwd`/badext.py

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > gpg =
  > hgext.gpg =
  > badext = $abspath
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
  *** failed to import extension badext2: No module named badext2
  Traceback (most recent call last):
  ImportError: No module named badext2
