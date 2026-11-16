"""With a libexpat already loaded with global symbols, the module still
calls its own XML_* functions (linked with -Bsymbolic on ELF platforms;
on macOS and Windows each library's calls bind to its own definitions).

    python -m unittest discover -s tests
"""

import ctypes
import ctypes.util
import subprocess
import sys
import textwrap
import unittest

LIBEXPAT = ctypes.util.find_library("expat")


@unittest.skipIf(LIBEXPAT is None, "no libexpat on this system")
class LinkingTests(unittest.TestCase):
    def test_own_functions_with_libexpat_loaded(self):
        code = textwrap.dedent(f"""
            import ctypes
            lib = ctypes.CDLL({LIBEXPAT!r}, mode=ctypes.RTLD_GLOBAL)
            lib.XML_ExpatVersion.restype = ctypes.c_char_p
            print(lib.XML_ExpatVersion().decode())
            import expat_rs
            # The module's own XML_ExpatVersion, looked up in it directly.
            own = ctypes.CDLL(expat_rs._pyexpat.__file__).XML_ExpatVersion
            own.restype = ctypes.c_char_p
            print(own().decode())
            print(expat_rs.EXPAT_VERSION)
            seen = []
            p = expat_rs.ParserCreate()
            p.StartElementHandler = lambda name, attrs: seen.append((name, attrs))
            p.Parse(b"<a x='1'><b/></a>", True)
            print(seen)
        """)
        done = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        theirs, own, reported, seen = done.stdout.splitlines()
        self.assertEqual(seen, "[('a', {'x': '1'}), ('b', {})]")
        if theirs == own:
            self.skipTest(f"the system libexpat has our version string, {own}")
        # Bound to libexpat's XML_ExpatVersion, the module would report its.
        self.assertEqual(reported, own)


if __name__ == "__main__":
    unittest.main()
