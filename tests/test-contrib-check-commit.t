Test the 'check-commit' script
==============================

Test long lines in header (should not be reported as too long description)

  $ cat > patch-with-long-header.diff << EOF
  > # HG changeset patch
  > # User timeless <timeless@mozdev.org>
  > # Date 1448911706 0
  > #      Mon Nov 30 19:28:26 2015 +0000
  > # Node ID c41cb6d2b7dbd62b1033727f8606b8c09fc4aa88
  > # Parent  42aa0e570eaa364a622bc4443b0bcb79b1100a58
  > # ClownJoke This is a veryly long header that should not be warned about because its not the description
  > transplant: use Oxford comma
  > 
  > diff --git a/hgext/transplant.py b/hgext/transplant.py
  > --- a/hgext/transplant.py
  > +++ b/hgext/transplant.py
  > @@ -599,7 +599,7 @@
  >              return
  >          if not (opts.get('source') or revs or
  >                  opts.get('merge') or opts.get('branch')):
  > -            raise error.Abort(_('no source URL, branch revision or revision '
  > +            raise error.Abort(_('no source URL, branch revision, or revision '
  >                                 'list provided'))
  >          if opts.get('all'):
  > EOF
  $ cat patch-with-long-header.diff | $TESTDIR/../contrib/check-commit
