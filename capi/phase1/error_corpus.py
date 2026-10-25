#!/usr/bin/env python3
"""Malformed inputs for deriving the expat-rs ErrorKind -> libexpat code map.

Usage: error_corpus.py <libexpat-errcodes-binary> <expat-rs-xmlwf> [--ns]
Writes each case to a temp dir, runs both, prints a table.
"""
import os
import re
import subprocess
import sys
import tempfile

CASES = {
    # structure
    "empty document": b"",
    "only whitespace": b"  \n",
    "unclosed element at EOF": b"<a><b>",
    "truncated start tag": b"<a><b",
    "truncated end tag": b"<a></a",
    "mismatched end tag": b"<a></b>",
    "end tag without start": b"</a>",
    "second root": b"<a/><b/>",
    "text after root": b"<a/>x",
    "text before root": b"x<a/>",
    "cdata outside root": b"<a/><![CDATA[x]]>",
    "char ref outside root": b"<a/>&#65;",
    "entity ref outside root": b"<a/>&amp;",
    # lexical
    "invalid char in content": b"<a>\x01</a>",
    "invalid char in attribute": b"<a x='\x01'/>",
    "invalid char in name": b"<a\x01/>",
    "bad name start": b"<1a/>",
    "lt in attribute value": b"<a x='<'/>",
    "unquoted attribute": b"<a x=1/>",
    "no equals in attribute": b"<a x '1'/>",
    "no space between attributes": b"<a x='1'y='2'/>",
    "duplicate attribute": b"<a x='1' x='2'/>",
    "cdata end in text": b"<a>]]></a>",
    "double hyphen in comment": b"<a><!-- a -- b --></a>",
    "unclosed comment": b"<a><!-- x",
    "unclosed cdata": b"<a><![CDATA[x",
    "unclosed pi": b"<a><?pi x",
    "pi without space": b"<a><?pi%x?></a>",
    "bare ampersand": b"<a>&</a>",
    "ampersand without semicolon": b"<a>&amp</a>",
    "bad char ref digits": b"<a>&#6a;</a>",
    "char ref to NUL": b"<a>&#0;</a>",
    "char ref surrogate": b"<a>&#xD800;</a>",
    "char ref too large": b"<a>&#x110000;</a>",
    "unknown <! construct": b"<a><!FOO></a>",
    # xml declaration / pi targets
    "xml decl not at start": b" <?xml version='1.0'?><a/>",
    "xml decl after comment": b"<!--c--><?xml version='1.0'?><a/>",
    "xml decl uppercase target": b"<?XML version='1.0'?><a/>",
    "reserved pi target in content": b"<a><?xmL x?></a>",
    "xml decl missing version": b"<?xml encoding='UTF-8'?><a/>",
    "xml decl bad version": b"<?xml version='2.0'?><a/>",
    "xml decl wrong order": b"<?xml version='1.0' standalone='yes' encoding='UTF-8'?><a/>",
    "xml decl bad standalone": b"<?xml version='1.0' standalone='maybe'?><a/>",
    "xml decl bad encoding name": b"<?xml version='1.0' encoding='_x'?><a/>",
    # entities
    "undefined entity": b"<a>&nope;</a>",
    "undefined entity in attribute": b"<a x='&nope;'/>",
    "recursive entity": b"<!DOCTYPE a [<!ENTITY e '&e;'>]><a>&e;</a>",
    "entity with unbalanced tags": b"<!DOCTYPE a [<!ENTITY e '<b>'>]><a>&e;</a>",
    "external entity in attribute": b"<!DOCTYPE a [<!ENTITY e SYSTEM 'x'>]><a x='&e;'/>",
    "unparsed entity in content": b"<!DOCTYPE a [<!NOTATION n SYSTEM 'n'><!ENTITY e SYSTEM 'x' NDATA n>]><a>&e;</a>",
    "lt via entity in attribute": b"<!DOCTYPE a [<!ENTITY e '&#60;'>]><a x='&e;'/>",
    "billion laughs": (b"<!DOCTYPE a [<!ENTITY a0 'aaaaaaaaaa'>" + b"".join(
        b"<!ENTITY a%d '%s'>" % (i, b"&a%d;" % (i - 1) * 10) for i in range(1, 8)) + b"]><a>&a7;</a>"),
    # dtd
    "doctype missing name": b"<!DOCTYPE><a/>",
    "doctype after root": b"<a/><!DOCTYPE a>",
    "bad element decl": b"<!DOCTYPE a [<!ELEMENT a>]><a/>",
    "mixed separators in content model": b"<!DOCTYPE a [<!ELEMENT a (b|c,d)>]><a/>",
    "bad attlist type": b"<!DOCTYPE a [<!ATTLIST a x NUTOKEN #IMPLIED>]><a/>",
    "pe reference inside declaration": b"<!DOCTYPE a [<!ENTITY % p 'x'><!ENTITY e '%p;'>]><a/>",
    "conditional section in internal subset": b"<!DOCTYPE a [<![INCLUDE[]]>]><a/>",
    "bad public id char": b"<!DOCTYPE a PUBLIC 'a{b' 'x'><a/>",
    "unclosed internal subset": b"<!DOCTYPE a [<!ELEMENT a ANY>",
    "junk in internal subset": b"<!DOCTYPE a [ junk ]><a/>",
    # encodings
    "invalid utf-8": b"<a>\xff</a>",
    "truncated utf-8": b"<a>\xc3",
    "unknown declared encoding": b"<?xml version='1.0' encoding='EBCDIC-XYZ'?><a/>",
    "utf-16 declared without bom": b"<?xml version='1.0' encoding='UTF-16'?><a/>",
}
NS_CASES = {
    "unbound element prefix": b"<p:a/>",
    "unbound attribute prefix": b"<a p:x='1'/>",
    "undeclaring a prefix": b"<a xmlns:p=''/>",
    "binding xml prefix wrongly": b"<a xmlns:xml='http://x'/>",
    "declaring xmlns prefix": b"<a xmlns:xmlns='http://x'/>",
    "binding xml namespace to other prefix": b"<a xmlns:x='http://www.w3.org/XML/1998/namespace'/>",
    "duplicate expanded attribute": b"<a xmlns:p='u' xmlns:q='u' p:x='1' q:x='2'/>",
    "colon in pi target": b"<a><?a:b x?></a>",
}


def main():
    libexpat, xmlwf = sys.argv[1], sys.argv[2]
    ns = "--ns" in sys.argv
    cases = NS_CASES if ns else CASES
    with tempfile.TemporaryDirectory() as d:
        paths = {}
        for i, (name, data) in enumerate(cases.items()):
            paths[name] = os.path.join(d, f"{i:03}.xml")
            open(paths[name], "wb").write(data)
        lib = {}
        out = subprocess.run([libexpat, *(["-n"] if ns else []), *paths.values()], capture_output=True, text=True).stdout
        for line in out.splitlines():
            f, code, msg, *_ = line.split("\t")
            lib[f] = (int(code), msg)
        for name, path in paths.items():
            r = subprocess.run([xmlwf, *(["--namespaces"] if ns else []), path], capture_output=True, text=True)
            ours = r.stderr.split(": ", 1)[-1].strip() if r.returncode else "OK"
            ours = re.sub(r" at Position \{[^}]*\}", "", ours)
            code, msg = lib[path]
            print(f"{name:40} {code:>2} {msg[:38]:38} | {ours[:70]}")


if __name__ == "__main__":
    main()
