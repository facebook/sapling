from __future__ import absolute_import
import pkgutil

# Indicate that hgext3rd.rust is a namspace package, and other python path
# directories may still be searched for hgext3rd.rust libraries.
__path__ = pkgutil.extend_path(__path__, __name__)
