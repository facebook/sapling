(Translations are optional)

#if gettext no-outer-repo

Test that translations are compiled and installed correctly.

Default encoding in tests is "ascii" and the translation is encoded
using the "replace" error handler:

  $ LANGUAGE=pt_BR hg tip
  abortado: n?o foi encontrado um reposit?rio em '$TESTTMP' (.hg n?o encontrado)!
  [255]

Using a more accommodating encoding:

  $ HGENCODING=UTF-8 LANGUAGE=pt_BR hg tip
  abortado: n\xc3\xa3o foi encontrado um reposit\xc3\xb3rio em '$TESTTMP' (.hg n\xc3\xa3o encontrado)! (esc)
  [255]

Different encoding:

  $ HGENCODING=Latin-1 LANGUAGE=pt_BR hg tip
  abortado: n\xe3o foi encontrado um reposit\xf3rio em '$TESTTMP' (.hg n\xe3o encontrado)! (esc)
  [255]

#endif

#if gettext

Test keyword search in translated help text:

  $ HGENCODING=UTF-8 LANGUAGE=de hg help -k Aktualisiert
  Themen:
  
   subrepos Unterarchive
  
  Befehle:
  
   pull   Ruft \xc3\x84nderungen von der angegebenen Quelle ab (esc)
   update Aktualisiert das Arbeitsverzeichnis (oder wechselt die Version)

#endif

Check Mercurial specific translation problems in each *.po files, and
tool itself by doctest

  $ cd "$TESTDIR"/../i18n
  $ $PYTHON check-translation.py *.po
  $ $PYTHON check-translation.py --doctest
  $ cd $TESTTMP

#if gettext

Check i18n cache isn't reused after encoding change:

  $ cat > $TESTTMP/encodingchange.py << EOF
  > from mercurial import encoding, registrar
  > from mercurial.i18n import _
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'encodingchange', norepo=True)
  > def encodingchange(ui):
  >     for encode in (b'ascii', b'UTF-8', b'ascii', b'UTF-8'):
  >         encoding.encoding = encode
  >         ui.write(b'%s\n' % _(b'(EXPERIMENTAL)'))
  > EOF

  $ LANGUAGE=ja hg --config extensions.encodingchange=$TESTTMP/encodingchange.py encodingchange
  (?????)
  (\xe5\xae\x9f\xe9\xa8\x93\xe7\x9a\x84\xe5\xae\x9f\xe8\xa3\x85) (esc)
  (?????)
  (\xe5\xae\x9f\xe9\xa8\x93\xe7\x9a\x84\xe5\xae\x9f\xe8\xa3\x85) (esc)

#endif
