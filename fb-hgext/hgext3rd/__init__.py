from __future__ import absolute_import
import pkgutil
# Indicate that hgext3rd is a namspace package, and other python path
# directories may still be searched for hgext3rd extensions.
__path__ = pkgutil.extend_path(__path__, __name__)

### IMPORTANT ###
# Do not add logic here that would diverge from mercurial's
# hgext3rd/__init__.py, the installed version of the file on debian systems is
# provided by mercurial itself; the packaging rules for remotefilelog explicitly
# ignore the file you're reading now.
