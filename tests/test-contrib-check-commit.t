Test the 'check-commit' script
==============================

A fine patch:

  $ cat > patch-with-long-header.diff << EOF
  > # HG changeset patch
  > # User timeless <timeless@mozdev.org>
  > # Date 1448911706 0
  > #      Mon Nov 30 19:28:26 2015 +0000
  > # Node ID c41cb6d2b7dbd62b1033727f8606b8c09fc4aa88
  > # Parent  42aa0e570eaa364a622bc4443b0bcb79b1100a58
  > # ClownJoke This is a veryly long header that should not be warned about because its not the description
  > bundle2: use Oxford comma (issue123) (BC)
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
  > 
  > + def blahblah(x):
  > +     pass
  > EOF
  $ cat patch-with-long-header.diff | $TESTDIR/../contrib/check-commit

This would normally be against the rules, but it's okay because that's
what tagging and signing looks like:

  $ cat > creates-a-tag.diff << EOF
  > # HG changeset patch
  > # User Augie Fackler <raf@durin42.com>
  > # Date 1484787778 18000
  > #      Wed Jan 18 20:02:58 2017 -0500
  > # Branch stable
  > # Node ID c177635e4acf52923bc3aa9f72a5b1ad1197b173
  > # Parent  a1dd2c0c479e0550040542e392e87bc91262517e
  > Added tag 4.1-rc for changeset a1dd2c0c479e
  > 
  > diff --git a/.hgtags b/.hgtags
  > --- a/.hgtags
  > +++ b/.hgtags
  > @@ -150,3 +150,4 @@ 438173c415874f6ac653efc1099dec9c9150e90f
  >  eab27446995210c334c3d06f1a659e3b9b5da769 4.0
  >  b3b1ae98f6a0e14c1e1ba806a6c18e193b6dae5c 4.0.1
  >  e69874dc1f4e142746ff3df91e678a09c6fc208c 4.0.2
  > +a1dd2c0c479e0550040542e392e87bc91262517e 4.1-rc
  > EOF
  $ $TESTDIR/../contrib/check-commit < creates-a-tag.diff

A patch with lots of errors:

  $ cat > patch-with-long-header.diff << EOF
  > # HG changeset patch
  > # User timeless
  > # Date 1448911706 0
  > #      Mon Nov 30 19:28:26 2015 +0000
  > # Node ID c41cb6d2b7dbd62b1033727f8606b8c09fc4aa88
  > # Parent  42aa0e570eaa364a622bc4443b0bcb79b1100a58
  > # ClownJoke This is a veryly long header that should not be warned about because its not the description
  > transplant/foo: this summary is way too long use Oxford comma (bc) (bug123) (issue 244)
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
  1: username is not an email address
   # User timeless
  7: summary keyword should be most user-relevant one-word command or topic
   transplant/foo: this summary is way too long use Oxford comma (bc) (bug123) (issue 244)
  7: (BC) needs to be uppercase
   transplant/foo: this summary is way too long use Oxford comma (bc) (bug123) (issue 244)
  7: use (issueDDDD) instead of bug
   transplant/foo: this summary is way too long use Oxford comma (bc) (bug123) (issue 244)
  7: no space allowed between issue and number
   transplant/foo: this summary is way too long use Oxford comma (bc) (bug123) (issue 244)
  7: summary line too long (limit is 78)
   transplant/foo: this summary is way too long use Oxford comma (bc) (bug123) (issue 244)
  [1]

A patch with other errors:

  $ cat > patch-with-long-header.diff << EOF
  > # HG changeset patch
  > # User timeless
  > # Date 1448911706 0
  > #      Mon Nov 30 19:28:26 2015 +0000
  > # Node ID c41cb6d2b7dbd62b1033727f8606b8c09fc4aa88
  > # Parent  42aa0e570eaa364a622bc4443b0bcb79b1100a58
  > # ClownJoke This is a veryly long header that should not be warned about because its not the description
  > This has no topic and ends with a period.
  > 
  > diff --git a/hgext/transplant.py b/hgext/transplant.py
  > --- a/hgext/transplant.py
  > +++ b/hgext/transplant.py
  > @@ -599,7 +599,7 @@
  >          if opts.get('all'):
  >  
  > 
  > +
  > + some = otherjunk
  > +
  > +
  > + def blah_blah(x):
  > +     pass
  > +
  >  
  > EOF
  $ cat patch-with-long-header.diff | $TESTDIR/../contrib/check-commit
  1: username is not an email address
   # User timeless
  7: don't capitalize summary lines
   This has no topic and ends with a period.
  7: summary line doesn't start with 'topic: '
   This has no topic and ends with a period.
  7: don't add trailing period on summary line
   This has no topic and ends with a period.
  [1]
