# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""server side extension to advertise pre-generated bundles to seed clones.

The extension essentially serves the content of a .hg/clonebundles.manifest
file to clients that request it.

The clonebundles.manifest file contains a list of URLs and attributes. URLs
hold pre-generated bundles that a client fetches and applies. After applying
the pre-generated bundle, the client will connect back to the original server
and pull data not in the pre-generated bundle.

Manifest File Format:

The manifest file contains a newline (\n) delimited list of entries.

Each line in this file defines an available bundle. Lines have the format:

    <URL> [<key>=<value]

That is, a URL followed by extra metadata describing it. Metadata keys and
values should be URL encoded.

This metadata is optional. It is up to server operators to populate this
metadata.

Keys in UPPERCASE are reserved for use by Mercurial. All non-uppercase keys
can be used by site installations.

The server operator is responsible for generating the bundle manifest file.

Metadata Attributes:

BUNDLESPEC
   A "bundle specification" string that describes the type of the bundle.

   These are string values that are accepted by the "--type" argument of
   `hg bundle`.

   The values are parsed in strict mode, which means they must be of the
   "<compression>-<type>" form. See
   mercurial.exchange.parsebundlespec() for more details.

   Clients will automatically filter out specifications that are unknown or
   unsupported so they won't attempt to download something that likely won't
   apply.

   The actual value doesn't impact client behavior beyond filtering:
   clients will still sniff the bundle type from the header of downloaded
   files.

REQUIRESNI
   Whether Server Name Indication (SNI) is required to connect to the URL.
   SNI allows servers to use multiple certificates on the same IP. It is
   somewhat common in CDNs and other hosting providers. Older Python
   versions do not support SNI. Defining this attribute enables clients
   with older Python versions to filter this entry.

   If this is defined, it is important to advertise a non-SNI fallback
   URL or clients running old Python releases may not be able to clone
   with the clonebundles facility.

   Value should be "true".
"""

from mercurial.i18n import _
from mercurial.node import nullid
from mercurial import (
    exchange,
    extensions,
    wireproto,
)

testedwith = 'internal'

def capabilities(orig, repo, proto):
    caps = orig(repo, proto)

    # Only advertise if a manifest exists. This does add some I/O to requests.
    # But this should be cheaper than a wasted network round trip due to
    # missing file.
    if repo.opener.exists('clonebundles.manifest'):
        caps.append('clonebundles')

    return caps

@wireproto.wireprotocommand('clonebundles', '')
def bundles(repo, proto):
    """Server command for returning info for available bundles to seed clones.

    Clients will parse this response and determine what bundle to fetch.

    Other extensions may wrap this command to filter or dynamically emit
    data depending on the request. e.g. you could advertise URLs for
    the closest data center given the client's IP address.
    """
    return repo.opener.tryread('clonebundles.manifest')

@exchange.getbundle2partsgenerator('clonebundlesadvertise', 0)
def advertiseclonebundlespart(bundler, repo, source, bundlecaps=None,
                              b2caps=None, heads=None, common=None,
                              cbattempted=None, **kwargs):
    """Inserts an output part to advertise clone bundles availability."""
    # Allow server operators to disable this behavior.
    # # experimental config: ui.clonebundleadvertise
    if not repo.ui.configbool('ui', 'clonebundleadvertise', True):
        return

    # Only advertise if a manifest is present.
    if not repo.opener.exists('clonebundles.manifest'):
        return

    # And when changegroup data is requested.
    if not kwargs.get('cg', True):
        return

    # And when the client supports clone bundles.
    if cbattempted is None:
        return

    # And when the client didn't attempt a clone bundle as part of this pull.
    if cbattempted:
        return

    # And when a full clone is requested.
    # Note: client should not send "cbattempted" for regular pulls. This check
    # is defense in depth.
    if common and common != [nullid]:
        return

    msg = _('this server supports the experimental "clone bundles" feature '
            'that should enable faster and more reliable cloning\n'
            'help test it by setting the "experimental.clonebundles" config '
            'flag to "true"')

    bundler.newpart('output', data=msg)

def extsetup(ui):
    extensions.wrapfunction(wireproto, '_capabilities', capabilities)
