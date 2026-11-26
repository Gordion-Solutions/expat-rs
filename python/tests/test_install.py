"""expat_rs.install(): each case runs in a fresh interpreter, since it
changes sys.modules for good.

    python -m unittest discover -s tests
"""

import os
import subprocess
import sys
import textwrap
import unittest


def run(code):
    """Run `code` in a fresh interpreter; return its stdout."""
    # UTF-8 output whatever the console's encoding (Windows).
    env = dict(os.environ, PYTHONIOENCODING="utf-8")
    done = subprocess.run(
        [sys.executable, "-c", textwrap.dedent(code)],
        capture_output=True,
        encoding="utf-8",
        env=env,
    )
    if done.returncode != 0:
        raise AssertionError(done.stderr)
    return done.stdout.strip()


class InstallTests(unittest.TestCase):
    def test_modules_replaced(self):
        out = run("""
            import expat_rs, sys
            expat_rs.install()
            expat_rs.install()  # again: nothing to do
            import pyexpat, xml.parsers.expat as expat
            from xml.parsers.expat import errors, model
            ours = expat_rs._pyexpat
            print(pyexpat is ours, expat.ParserCreate is ours.ParserCreate,
                  errors is ours.errors, model is ours.model,
                  sys.modules["xml.parsers.expat.errors"] is ours.errors,
                  type(pyexpat.expat_CAPI).__name__)
        """)
        self.assertEqual(out, "True True True True True PyCapsule")

    def test_elementtree_c_accelerator(self):
        # _elementtree builds its parsers from the capsule's functions;
        # the version it reports is the interpreter's libexpat header's.
        out = run("""
            import expat_rs
            expat_rs.install()
            import xml.etree.ElementTree as ET
            p = ET.XMLParser()
            print(type(p).__module__)
            root = ET.fromstring(
                '<!DOCTYPE d [<!ENTITY e "<![CDATA[&x;]]>">]><d a="1">&e;<b/></d>')
            print(root.text, root.get("a"), [c.tag for c in root])
            try:
                ET.fromstring("<a><b></a>")
            except ET.ParseError as e:
                print(e, e.code)
            print(ET.fromstring(
                b'<?xml version="1.0" encoding="iso-8859-5"?><a>\\xd0</a>').text)
        """)
        self.assertEqual(
            out.splitlines(),
            [
                "xml.etree.ElementTree",
                "&x; 1 ['b']",
                "mismatched tag: line 1, column 8 7",
                "а",
            ],
        )

    def test_sax_and_minidom(self):
        out = run("""
            import expat_rs
            expat_rs.install()
            import xml.sax, xml.dom.minidom, xml.sax.handler, plistlib
            class H(xml.sax.handler.ContentHandler):
                def startElement(self, name, attrs):
                    print("start", name, dict(attrs))
            xml.sax.parseString(b"<a x='1'/>", H())
            print(xml.dom.minidom.parseString("<a><b>t</b></a>").documentElement.toxml())
            print(plistlib.loads(plistlib.dumps({"k": [1, "v"]})))
        """)
        self.assertEqual(
            out.splitlines(),
            ["start a {'x': '1'}", "<a><b>t</b></a>", "{'k': [1, 'v']}"],
        )

    def test_reparse_deferral_default(self):
        # On by default only if the interpreter's own pyexpat can turn it
        # off (CPython 3.10.14, 3.11.9, 3.12.3 and later).
        out = run("""
            import pyexpat
            supported = hasattr(pyexpat.ParserCreate(), "SetReparseDeferralEnabled")
            import expat_rs
            expat_rs.install()
            import xml.parsers.expat as expat
            p = expat.ParserCreate()
            print(p.GetReparseDeferralEnabled() == supported,
                  p.ExternalEntityParserCreate(None).GetReparseDeferralEnabled() == supported)
            import xml.etree.ElementTree as ET
            events = ET.XMLPullParser(["end"])
            events.feed("<a><b/>")
            print([e.tag for _, e in events.read_events()])
        """)
        self.assertEqual(out.splitlines(), ["True True", "['b']"])

    def test_too_late(self):
        out = run("""
            import xml.etree.ElementTree, expat_rs
            try:
                expat_rs.install()
            except RuntimeError as e:
                print(e)
            import pyexpat
            print(pyexpat is expat_rs._pyexpat)
        """)
        self.assertEqual(
            out.splitlines(),
            [
                "expat_rs.install() must be called before these modules are imported: "
                "_elementtree, xml.etree.ElementTree",
                "False",
            ],
        )


if __name__ == "__main__":
    unittest.main()
