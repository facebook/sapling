try:
    import coverage
    if hasattr(coverage, 'process_startup'):
        coverage.process_startup()
except ImportError:
    pass
