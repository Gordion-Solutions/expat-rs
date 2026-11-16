"""Arguments pyexpat declares `bool` accept what the standard library's
module accepts: any object, except `Parse`'s isfinal before Python 3.12,
which must be an integer.

    python -m unittest discover -s tests
"""

import sys
import unittest

import expat_rs


class BoolArgumentTests(unittest.TestCase):
    def test_isfinal(self):
        for value in (1, 0, True, False):
            expat_rs.ParserCreate().Parse(b"<a/>", value)
        for value in (None, "x", 2.5):
            p = expat_rs.ParserCreate()
            if sys.version_info >= (3, 12):
                p.Parse(b"<a/>", value)
            else:
                with self.assertRaisesRegex(TypeError, "cannot be interpreted as an integer"):
                    p.Parse(b"<a/>", value)

    def test_isfinal_by_value(self):
        p = expat_rs.ParserCreate()
        p.Parse(b"<a>", 0)
        with self.assertRaises(expat_rs.ExpatError):
            p.Parse(b"", 1)

    def test_flags(self):
        p = expat_rs.ParserCreate()
        p.SetReparseDeferralEnabled(0)
        self.assertFalse(p.GetReparseDeferralEnabled())
        p.SetReparseDeferralEnabled("yes")
        self.assertTrue(p.GetReparseDeferralEnabled())
        p.UseForeignDTD(1)
        p.UseForeignDTD(None)


if __name__ == "__main__":
    unittest.main()
