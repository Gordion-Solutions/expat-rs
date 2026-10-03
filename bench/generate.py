#!/usr/bin/env python3
"""Generate the benchmark documents (25-56 MB each) into a directory.

Usage: generate.py <out-dir>
"""
import os
import random
import sys

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
random.seed(1)
words = "the quick brown fox jumps over lazy dog lorem ipsum dolor sit amet consectetur".split()


def text(n):
    return " ".join(random.choice(words) for _ in range(n))


# Data records: many small elements with attributes (exports, configs).
with open(f"{out}/records.xml", "w") as f:
    f.write("<?xml version='1.0' encoding='UTF-8'?>\n<records>\n")
    for i in range(400_000):
        f.write(f'  <record id="{i}" type="t{i%7}" active="{"yes" if i%3 else "no"}">'
                f'<name>{text(3)}</name><value unit="kg">{i*1.5}</value></record>\n')
    f.write("</records>\n")

# Text-heavy articles with references.
with open(f"{out}/articles.xml", "w") as f:
    f.write("<?xml version='1.0'?>\n<articles>\n")
    for i in range(20_000):
        f.write(f"<article n='{i}'><title>{text(8)}</title>")
        for _ in range(5):
            f.write(f"<p>{text(60)} &amp; {text(20)} &#169; {text(10)}</p>")
        f.write("</article>\n")
    f.write("</articles>\n")

# Namespaced Atom-style feed.
with open(f"{out}/feed.xml", "w") as f:
    f.write("<?xml version='1.0'?>\n<feed xmlns='http://www.w3.org/2005/Atom' "
            "xmlns:media='http://search.yahoo.com/mrss/'>\n")
    for i in range(150_000):
        f.write(f"<entry><id>urn:uuid:{i}</id><title type='text'>{text(6)}</title>"
                f"<link rel='alternate' href='https://example.org/{i}'/>"
                f"<media:thumbnail url='https://example.org/{i}.jpg' width='120' height='90'/>"
                f"<summary>{text(25)}</summary></entry>\n")
    f.write("</feed>\n")

# Multilingual, non-ASCII text.
uni = ["日本語のテキスト", "Ελληνικά κείμενα", "русский текст", "العربية", "emoji 🎉✨", "Ünïcödé façade naïve"]
with open(f"{out}/unicode.xml", "w") as f:
    f.write("<?xml version='1.0' encoding='UTF-8'?>\n<doc>\n")
    for i in range(300_000):
        f.write(f"<s lang='x{i%6}'>{random.choice(uni)} {random.choice(uni)} {text(4)}</s>\n")
    f.write("</doc>\n")
