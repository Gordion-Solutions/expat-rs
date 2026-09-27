//! DOCTYPE and internal-subset syntax (W3C XML 1.0 §2.8, §3.2–§4.7).
//! Well-formedness only: nothing here checks validity against the DTD.

use expat_rs::{Edition, Parser, XmlError};

fn parse(src: &str) -> Result<(), XmlError> {
    let mut p = Parser::new(src);
    while p.next_event()?.is_some() {}
    Ok(())
}

fn with_subset(subset: &str) -> String {
    format!("<!DOCTYPE doc [\n{subset}\n]>\n<doc/>")
}

fn ok(subset: &str) {
    parse(&with_subset(subset)).unwrap_or_else(|e| panic!("rejected {subset:?}: {e}"));
}

fn bad(subset: &str) {
    assert!(parse(&with_subset(subset)).is_err(), "accepted {subset:?}");
}

// ─── doctypedecl [28] ───────────────────────────────────────────────────────

#[test]
fn doctype_forms() {
    for doc in [
        "<!DOCTYPE doc><doc/>",
        "<!DOCTYPE doc SYSTEM 'doc.dtd'><doc/>",
        "<!DOCTYPE doc PUBLIC '-//X//DTD Y//EN' \"doc.dtd\"><doc/>",
        "<!DOCTYPE doc SYSTEM 'doc.dtd' [ ]><doc/>",
        "<!DOCTYPE doc [] ><doc/>",
    ] {
        parse(doc).unwrap_or_else(|e| panic!("rejected {doc:?}: {e}"));
    }
    for doc in [
        "<!DOCTYPEdoc><doc/>",                        // S required
        "<!DOCTYPE doc SYSTEM><doc/>",                // literal required
        "<!DOCTYPE doc PUBLIC 'x'><doc/>",            // system literal required
        "<!DOCTYPE doc PUBLIC 'a' 'b'extra><doc/>",
        "<!DOCTYPE doc SYSTEM'x'><doc/>",
        "<!DOCTYPE doc PUBLIC 'bad\\char' 'x'><doc/>", // PubidChar
        "<!DOCTYPE doc [ <!ELEMENT doc EMPTY> <doc/>", // unclosed subset
    ] {
        assert!(parse(doc).is_err(), "accepted {doc:?}");
    }
}

#[test]
fn bracket_in_entity_value_does_not_end_subset() {
    parse("<!DOCTYPE doc [<!ENTITY rsqb \"]\">]><doc>&rsqb;</doc>").expect("']' inside a literal");
}

#[test]
fn subset_rejects_stray_content() {
    bad("hello");
    bad("<doc/>");
    bad("<![INCLUDE[ <!ELEMENT doc EMPTY> ]]>"); // conditional sections: external subset only
    bad("<?xml version='1.0'?>");                  // reserved PI target
}

#[test]
fn subset_pis_comments_pe_refs() {
    ok("<?pi data?> <!-- comment --> %pe;");
    bad("<!-- a -- b -->");
    bad("%pe");
    bad("% pe;");
}

// ─── elementdecl [45]–[51] ─────────────────────────────────────────────────

#[test]
fn element_declarations() {
    ok("<!ELEMENT doc EMPTY>");
    ok("<!ELEMENT doc ANY>");
    ok("<!ELEMENT doc (#PCDATA)>");
    ok("<!ELEMENT doc (#PCDATA)*>");
    ok("<!ELEMENT doc ( #PCDATA | a | b )*>");
    ok("<!ELEMENT doc (a, (b | c)*, d?)+>");
    ok("<!ELEMENT doc (a)>");
    bad("<!ELEMENT doc>");
    bad("<!ELEMENT doc empty>");
    bad("<!ELEMENT doc ()>");
    bad("<!ELEMENT doc (a | b, c)>");   // mixed separators
    bad("<!ELEMENT doc (#PCDATA | a)>"); // names need ')*'
    bad("<!ELEMENT doc (a, #PCDATA)>");  // #PCDATA only first
    bad("<!ELEMENT doc (a)) >");
    bad("<!ELEMENT doc (a) - (b)>");     // SGML exceptions
    bad("<!ELEMENTdoc EMPTY>");
}

// ─── AttlistDecl [52]–[60] ──────────────────────────────────────────────────

#[test]
fn attlist_declarations() {
    ok("<!ATTLIST doc>");
    ok("<!ATTLIST doc a CDATA #IMPLIED b ID #REQUIRED c NMTOKENS 'x y'>");
    ok("<!ATTLIST doc a (x | y | 1) 'x' b NOTATION (n) #FIXED 'n'>");
    ok("<!ATTLIST doc a IDREFS #IMPLIED b ENTITIES #IMPLIED>");
    bad("<!ATTLIST doc a NUTOKENS #IMPLIED>");
    bad("<!ATTLIST doc a CDATA>");
    bad("<!ATTLIST doc a CDATA #FIXED>");
    bad("<!ATTLIST doc a CDATA #IMPLIEDb CDATA #IMPLIED>");
    bad("<!ATTLIST doc a (x, y) 'x'>");
    bad("<!ATTLIST doc a () 'x'>");
    bad("<!ATTLIST doc a NOTATION(n) 'n'>");
    bad("<!ATTLIST doc a CDATA '<'>");
}

// ─── EntityDecl [70]–[76] ───────────────────────────────────────────────────

#[test]
fn entity_declarations() {
    ok("<!ENTITY e 'text &amp; &#65;'>");
    ok("<!ENTITY % p \"text\">");
    ok("<!ENTITY e SYSTEM 'e.xml'>");
    ok("<!ENTITY e PUBLIC '-//X//Y//EN' 'e.xml' NDATA gif>");
    ok("<!ENTITY % p PUBLIC '-//X//Y//EN' 'p.ent'>");
    bad("<!ENTITY e>");
    bad("<!ENTITY e 'unterminated>");
    bad("<!ENTITY e 'has %pe; inside'>"); // PEs in Internal Subset WFC
    bad("<!ENTITY e 'bad &ref'>");
    bad("<!ENTITY e 'bad &#0;'>");
    bad("<!ENTITY %p 'x'>");
    bad("<!ENTITY % p SYSTEM 'p' NDATA gif>");
    bad("<!ENTITY e SYSTEM 'e'NDATA gif>");
}

// ─── NotationDecl [82], [83] ────────────────────────────────────────────────

#[test]
fn notation_declarations() {
    ok("<!NOTATION n SYSTEM 'viewer'>");
    ok("<!NOTATION n PUBLIC '-//X//NOTATION Y//EN'>");
    ok("<!NOTATION n PUBLIC '-//X//NOTATION Y//EN' 'viewer'>");
    bad("<!NOTATION n>");
    bad("<!NOTATION n 'viewer'>");
}

// ─── Edition rules apply inside the DTD ────────────────────────────────────

#[test]
fn edition_name_rules_apply_in_subset() {
    let doc = with_subset("<?p\u{132} x?>");
    parse(&doc).expect("5th ed. accepts U+0132 in a PI target");
    let mut p = Parser::new(&doc).with_edition(Edition::Fourth);
    let r = (|| { while p.next_event()?.is_some() {} Ok::<_, XmlError>(()) })();
    assert!(r.is_err(), "4th ed. rejects U+0132 in a PI target");
}
