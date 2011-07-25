try:
    import coverage
    getattr(coverage, 'process_startup', lambda: None)()
except ImportError:
    pass
