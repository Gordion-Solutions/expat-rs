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
