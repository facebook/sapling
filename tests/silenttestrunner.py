import unittest, sys, os

def main(modulename):
    '''run the tests found in module, printing nothing when all tests pass'''
    module = sys.modules[modulename]
    suite = unittest.defaultTestLoader.loadTestsFromModule(module)
    results = unittest.TestResult()
    suite.run(results)
    if results.errors or results.failures:
        for tc, exc in results.errors:
            print 'ERROR:', tc
            print
            sys.stdout.write(exc)
        for tc, exc in results.failures:
            print 'FAIL:', tc
            print
            sys.stdout.write(exc)
        sys.exit(1)

if os.environ.get('SILENT_BE_NOISY'):
    main = unittest.main
