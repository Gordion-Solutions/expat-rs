"""A parser may be used and freed on threads other than its own, as with
the standard library's pyexpat.

    python -m unittest discover -s tests
"""

import gc
import threading
import unittest

import expat_rs


def on_thread(f):
    out = []
    t = threading.Thread(target=lambda: out.append(f()))
    t.start()
    t.join()
    return out[0]


class ThreadTests(unittest.TestCase):
    def test_parse_on_another_thread(self):
        p = expat_rs.ParserCreate()
        seen = []
        p.StartElementHandler = lambda name, attrs: seen.append(name)
        self.assertEqual(on_thread(lambda: p.Parse(b"<a><b/></a>", True)), 1)
        self.assertEqual(seen, ["a", "b"])

    def test_made_on_another_thread(self):
        p = on_thread(expat_rs.ParserCreate)
        seen = []
        p.EndElementHandler = seen.append
        p.Parse(b"<a/>", True)
        self.assertEqual(seen, ["a"])
        del p
        gc.collect()

    def test_freed_on_another_thread(self):
        box = [expat_rs.ParserCreate()]
        sub = box[0].ExternalEntityParserCreate(None)

        def free():
            del box[:]
            gc.collect()

        on_thread(free)
        on_thread(lambda: sub.Parse(b"<x/>", True))


if __name__ == "__main__":
    unittest.main()
