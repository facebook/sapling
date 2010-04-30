"""Compatability functions for old Mercurial versions."""

def progress(ui, *args, **kwargs):
    """Shim for progress on hg < 1.4. Remove when 1.3 is dropped."""
    getattr(ui, 'progress', lambda *x, **kw: None)(*args, **kwargs)
