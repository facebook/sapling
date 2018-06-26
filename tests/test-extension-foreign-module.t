An extension itself cannot be outside the main hgext:

  $ mkdir $TESTTMP/hgext
  $ cat > $TESTTMP/hgext/__init__.py << EOF
  > from __future__ import absolute_import
  > import pkgutil
  > __path__ = pkgutil.extend_path(__path__, __name__)
  > EOF

  $ cat > $TESTTMP/hgext/alienext.py << EOF
  > from __future__ import absolute_import
  > def uisetup(ui):
  >     ui.write(('alienext loaded: %s\n' % __file__))
  > EOF

  $ PYTHONPATH=$TESTTMP:${PYTHONPATH:-/dev/null} hg --config extensions.alienext= help -e alienext 2>&1 | tail -1
  mercurial.error.ForeignImportError: hgext.alienext: $TESTTMP/hgext/alienext.py* lives outside * (glob)

An allowed extension cannot indirectly import a module inside a foreign hgext directory:

  $ cat > $TESTTMP/allowedext1.py << EOF
  > import alienext
  > EOF

  $ PYTHONPATH=$TESTTMP:$TESTTMP/hgext:${PYTHONPATH:-/dev/null} hg --config extensions.allowedext1= help -e allowedext1 2>&1 | tail -1
  mercurial.error.ForeignImportError: alienext: $TESTTMP/hgext/alienext.py* lives outside * (glob)

Modules outside hgext are not protected by this check. This is for compatibility.
(Therefore, hgext.extlib is encouraged to be used for dependent modules that are not hg extensions)

  $ touch $TESTTMP/alienoutside.py
  $ cat > $TESTTMP/allowedext2.py << EOF
  > import alienoutside
  > EOF

  $ PYTHONPATH=$TESTTMP:${PYTHONPATH:-/dev/null} hg --config extensions.allowedext2= help -e allowedext2
  allowedext2 extension - no help text available
  
  no commands defined

