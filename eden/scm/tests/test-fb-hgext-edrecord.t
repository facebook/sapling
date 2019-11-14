  $ cat << '__EOF__' >> $HGRCPATH
  > [ui]
  > interface=editor
  > [extensions]
  > edrecord=
  > __EOF__

  $ hg init repo
  $ cd repo

Setup:

  $ echo "
  > a" >> a
  $ touch b
  $ hg add a b

  $ hg status a b
  A a
  A b

Commit interactively
Include nothing in the commit

  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > : > "$1"
  > __EOF__
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit -i -m "initial commit"
  no changes to record

Test the 'ui.editor.chunkselector' config option

  $ HGEDITOR="" hg commit -i -m "initial commit" --config ui.editor.chunkselector="\"sh\" \"`pwd`/editor.sh\""
  no changes to record

Only include changes in a to the commit

  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > echo "\
  > diff --git a/a b/a
  > new file mode 100644
  > --- /dev/null
  > +++ b/a
  > @@ -0,0 +1,1 @@
  > +
  > +a
  > # Don't include b in this commit
  > #" > "$1"
  > __EOF__
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit -i -m "initial commit"

Check only a was committed but b still has changes

  $ hg status a b
  A b

Include all uncommitted changes in the next commit

  $ HGEDITOR=: hg commit -i -m "second commit"
  $ hg status a b

Test empty lines are handled
Note: empty lines are never actually part of a patch in mercurial,
but sometimes the editor being used to edit the patch may strip out trailing
whitespace on lines. Context lines begin with a single space, and display the
rest of the line. If an empty line is part of the context and the editor strips
out trailing whitespace, then the editor will strip out the character that
indicates that that line is context. Without handling this case, the patch
parser will get confused when it sees an empty line without any initial
character that describes what the line's function is, and will abort.

  $ echo "a" >> a
  $ hg status a b
  M a
  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > echo "\
  > diff --git a/a b/a
  > --- a/a
  > +++ b/a
  > @@ -1,2 +1,3 @@
  > ""
  >  a
  > +a
  > #" > "$1"
  > __EOF__
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit -i -m "third commit"
  $ hg status a b
