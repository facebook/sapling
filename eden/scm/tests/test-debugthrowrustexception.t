  $ configure modern
  $ newserver master

Test debugthrowrustexception error formatting
  $ hg debugthrowrustexception 2>&1 | egrep -v '^  '
  \*\* Mercurial Distributed SCM * has crashed: (glob)
  Traceback (most recent call last):
  *RustError: intentional error for debugging with message 'intentional_error' (glob)
  $ hg debugthrowrustexception --traceback 2>&1 | egrep -v '^  '
  Traceback (most recent call last):
  *RustError: intentional error for debugging with message 'intentional_error' (glob)
  error has type name taggederror::IntentionalError and fault None
  \*\* Mercurial Distributed SCM * has crashed: (glob)
  Traceback (most recent call last):
  *RustError: intentional error for debugging with message 'intentional_error' (glob)
