# expat-rs for Python

A memory-safe drop-in for Python's XML parser, `xml.parsers.expat`
(CPython's `pyexpat` module), running on
[expat-rs](https://github.com/Gordion-Solutions/expat-rs), an XML parser
written in Rust.

Work in progress: not yet released.

```python
from expat_rs import ParserCreate

p = ParserCreate()
p.StartElementHandler = lambda name, attrs: print(name, attrs)
p.Parse(b"<doc a='1'/>", True)
```

## Making the standard library use it

```python
import expat_rs
expat_rs.install()

import xml.etree.ElementTree as ET   # now parses with expat-rs
```

`install()` puts this module in place of `pyexpat` and `xml.parsers.expat`,
and gives ElementTree's C accelerator (`_elementtree`) the same table of
parser functions that `pyexpat` gives it. ElementTree, SAX, minidom,
pulldom, plistlib, xmlrpc and anything else built on `xml.parsers.expat`
then parse with expat-rs.

Call it at start-up, before anything imports those modules: it raises
`RuntimeError` if one of them already has, since that module would go on
using libexpat. On Python 3.10 and 3.11, `_elementtree` can't be made to
switch once imported.

On interpreters from before libexpat 2.6's reparse deferral (older than
CPython 3.10.14, 3.11.9 and 3.12.3), whose standard library can't turn it
off, `install()` makes parsers start with it off, as their libexpat had it.


## Threads

As with `pyexpat`, a parser may be used from any thread, though not from
two at once.

## Testing

```sh
pip install expat-rs     # or: maturin develop, from this directory
python -m unittest discover -s tests
python tests/stdlib.py   # CPython's XML test suites, through install()
```

`tests/stdlib.py` runs the interpreter's own test suites for `pyexpat`,
ElementTree, SAX, minidom, pulldom, plistlib and xmlrpc with `install()` in
effect; the interpreter needs its `test` package (or a CPython checkout's
`Lib` directory of the same version on `PYTHONPATH`). CI does this on
Linux, macOS and Windows for every supported CPython version.
