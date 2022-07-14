Common directory for Mononoke features implementations
=====
This directory contains source control features implemented by combining together repo facets.  Typically these are implemented in terms of repo: &impl FacetRef + FacetRef + ... or for legacy code repo: &BlobRepo.  These do not hold state, but rather store their state via repo attributes (e.g. microwave reads from changesets/filenodes and stores in blobstore or vice-versa).

TODO(mitrandir): move more features into this directory
