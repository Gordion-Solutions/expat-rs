//! Error kinds against libexpat: each malformed input must produce the
//! ErrorKind matching the error code libexpat 2.8.2 reports for it.
//! GENERATED from capi/phase1/error_corpus.py and libexpat's output; known
//! divergences are listed in KNOWN_DIVERGENCES with a reason.

use expat_rs::{decode, ErrorKind, Parser, XmlError};

/// (case, input, namespace processing, libexpat's error as an ErrorKind)
const CASES: &[(&str, &[u8], bool, ErrorKind)] = &[
    ("empty document", b"", false, ErrorKind::NoElements),
    ("only whitespace", b"  \x0a", false, ErrorKind::NoElements),
    ("unclosed element at EOF", b"<a><b>", false, ErrorKind::NoElements),
    ("truncated start tag", b"<a><b", false, ErrorKind::UnclosedToken),
    ("truncated end tag", b"<a></a", false, ErrorKind::UnclosedToken),
    ("mismatched end tag", b"<a></b>", false, ErrorKind::TagMismatch),
    ("end tag without start", b"</a>", false, ErrorKind::InvalidToken),
    ("second root", b"<a/><b/>", false, ErrorKind::JunkAfterDocElement),
    ("text after root", b"<a/>x", false, ErrorKind::JunkAfterDocElement),
    ("text before root", b"x<a/>", false, ErrorKind::InvalidToken),
    ("cdata outside root", b"<a/><![CDATA[x]]>", false, ErrorKind::JunkAfterDocElement),
    ("char ref outside root", b"<a/>&#65;", false, ErrorKind::InvalidToken),
    ("entity ref outside root", b"<a/>&amp;", false, ErrorKind::InvalidToken),
    ("invalid char in content", b"<a>\x01</a>", false, ErrorKind::InvalidToken),
    ("invalid char in attribute", b"<a x='\x01'/>", false, ErrorKind::InvalidToken),
    ("invalid char in name", b"<a\x01/>", false, ErrorKind::InvalidToken),
    ("bad name start", b"<1a/>", false, ErrorKind::InvalidToken),
    ("lt in attribute value", b"<a x='<'/>", false, ErrorKind::InvalidToken),
    ("unquoted attribute", b"<a x=1/>", false, ErrorKind::InvalidToken),
    ("no equals in attribute", b"<a x '1'/>", false, ErrorKind::InvalidToken),
    ("no space between attributes", b"<a x='1'y='2'/>", false, ErrorKind::InvalidToken),
    ("duplicate attribute", b"<a x='1' x='2'/>", false, ErrorKind::DuplicateAttribute),
    ("cdata end in text", b"<a>]]></a>", false, ErrorKind::InvalidToken),
    ("double hyphen in comment", b"<a><!-- a -- b --></a>", false, ErrorKind::InvalidToken),
    ("unclosed comment", b"<a><!-- x", false, ErrorKind::UnclosedToken),
    ("unclosed cdata", b"<a><![CDATA[x", false, ErrorKind::UnclosedCdataSection),
    ("unclosed pi", b"<a><?pi x", false, ErrorKind::UnclosedToken),
    ("pi without space", b"<a><?pi%x?></a>", false, ErrorKind::InvalidToken),
    ("bare ampersand", b"<a>&</a>", false, ErrorKind::InvalidToken),
    ("ampersand without semicolon", b"<a>&amp</a>", false, ErrorKind::InvalidToken),
    ("bad char ref digits", b"<a>&#6a;</a>", false, ErrorKind::InvalidToken),
    ("char ref to NUL", b"<a>&#0;</a>", false, ErrorKind::BadCharRef),
    ("char ref surrogate", b"<a>&#xD800;</a>", false, ErrorKind::BadCharRef),
    ("char ref too large", b"<a>&#x110000;</a>", false, ErrorKind::BadCharRef),
    ("unknown <! construct", b"<a><!FOO></a>", false, ErrorKind::InvalidToken),
    ("xml decl not at start", b" <?xml version='1.0'?><a/>", false, ErrorKind::MisplacedXmlPi),
    ("xml decl after comment", b"<!--c--><?xml version='1.0'?><a/>", false, ErrorKind::MisplacedXmlPi),
    ("xml decl uppercase target", b"<?XML version='1.0'?><a/>", false, ErrorKind::InvalidToken),
    ("reserved pi target in content", b"<a><?xmL x?></a>", false, ErrorKind::InvalidToken),
    ("xml decl missing version", b"<?xml encoding='UTF-8'?><a/>", false, ErrorKind::XmlDecl),
    ("xml decl wrong order", b"<?xml version='1.0' standalone='yes' encoding='UTF-8'?><a/>", false, ErrorKind::XmlDecl),
    ("xml decl bad standalone", b"<?xml version='1.0' standalone='maybe'?><a/>", false, ErrorKind::XmlDecl),
    ("xml decl bad encoding name", b"<?xml version='1.0' encoding='_x'?><a/>", false, ErrorKind::XmlDecl),
    ("undefined entity", b"<a>&nope;</a>", false, ErrorKind::UndefinedEntity),
    ("undefined entity in attribute", b"<a x='&nope;'/>", false, ErrorKind::UndefinedEntity),
    ("recursive entity", b"<!DOCTYPE a [<!ENTITY e '&e;'>]><a>&e;</a>", false, ErrorKind::RecursiveEntityRef),
    ("entity with unbalanced tags", b"<!DOCTYPE a [<!ENTITY e '<b>'>]><a>&e;</a>", false, ErrorKind::AsyncEntity),
    ("external entity in attribute", b"<!DOCTYPE a [<!ENTITY e SYSTEM 'x'>]><a x='&e;'/>", false, ErrorKind::AttributeExternalEntityRef),
    ("unparsed entity in content", b"<!DOCTYPE a [<!NOTATION n SYSTEM 'n'><!ENTITY e SYSTEM 'x' NDATA n>]><a>&e;</a>", false, ErrorKind::BinaryEntityRef),
    ("lt via entity in attribute", b"<!DOCTYPE a [<!ENTITY e '&#60;'>]><a x='&e;'/>", false, ErrorKind::InvalidToken),
    ("billion laughs", b"<!DOCTYPE a [<!ENTITY a0 'aaaaaaaaaa'><!ENTITY a1 '&a0;&a0;&a0;&a0;&a0;&a0;&a0;&a0;&a0;&a0;'><!ENTITY a2 '&a1;&a1;&a1;&a1;&a1;&a1;&a1;&a1;&a1;&a1;'><!ENTITY a3 '&a2;&a2;&a2;&a2;&a2;&a2;&a2;&a2;&a2;&a2;'><!ENTITY a4 '&a3;&a3;&a3;&a3;&a3;&a3;&a3;&a3;&a3;&a3;'><!ENTITY a5 '&a4;&a4;&a4;&a4;&a4;&a4;&a4;&a4;&a4;&a4;'><!ENTITY a6 '&a5;&a5;&a5;&a5;&a5;&a5;&a5;&a5;&a5;&a5;'><!ENTITY a7 '&a6;&a6;&a6;&a6;&a6;&a6;&a6;&a6;&a6;&a6;'>]><a>&a7;</a>", false, ErrorKind::AmplificationLimitBreach),
    ("doctype missing name", b"<!DOCTYPE><a/>", false, ErrorKind::InvalidToken),
    ("doctype after root", b"<a/><!DOCTYPE a>", false, ErrorKind::JunkAfterDocElement),
    ("bad element decl", b"<!DOCTYPE a [<!ELEMENT a>]><a/>", false, ErrorKind::Syntax),
    ("mixed separators in content model", b"<!DOCTYPE a [<!ELEMENT a (b|c,d)>]><a/>", false, ErrorKind::Syntax),
    ("bad attlist type", b"<!DOCTYPE a [<!ATTLIST a x NUTOKEN #IMPLIED>]><a/>", false, ErrorKind::Syntax),
    ("pe reference inside declaration", b"<!DOCTYPE a [<!ENTITY % p 'x'><!ENTITY e '%p;'>]><a/>", false, ErrorKind::ParamEntityRef),
    ("conditional section in internal subset", b"<!DOCTYPE a [<![INCLUDE[]]>]><a/>", false, ErrorKind::Syntax),
    ("bad public id char", b"<!DOCTYPE a PUBLIC 'a{b' 'x'><a/>", false, ErrorKind::PublicId),
    ("unclosed internal subset", b"<!DOCTYPE a [<!ELEMENT a ANY>", false, ErrorKind::NoElements),
    ("junk in internal subset", b"<!DOCTYPE a [ junk ]><a/>", false, ErrorKind::Syntax),
    ("invalid utf-8", b"<a>\xff</a>", false, ErrorKind::InvalidToken),
    ("truncated utf-8", b"<a>\xc3", false, ErrorKind::PartialChar),
    ("unknown declared encoding", b"<?xml version='1.0' encoding='EBCDIC-XYZ'?><a/>", false, ErrorKind::UnknownEncoding),
    ("utf-16 declared without bom", b"<?xml version='1.0' encoding='UTF-16'?><a/>", false, ErrorKind::IncorrectEncoding),
    ("unbound element prefix", b"<p:a/>", true, ErrorKind::UnboundPrefix),
    ("unbound attribute prefix", b"<a p:x='1'/>", true, ErrorKind::UnboundPrefix),
    ("undeclaring a prefix", b"<a xmlns:p=''/>", true, ErrorKind::UndeclaringPrefix),
    ("binding xml prefix wrongly", b"<a xmlns:xml='http://x'/>", true, ErrorKind::ReservedPrefixXml),
    ("declaring xmlns prefix", b"<a xmlns:xmlns='http://x'/>", true, ErrorKind::ReservedPrefixXmlns),
    ("binding xml namespace to other prefix", b"<a xmlns:x='http://www.w3.org/XML/1998/namespace'/>", true, ErrorKind::ReservedNamespaceUri),
    ("duplicate expanded attribute", b"<a xmlns:p='u' xmlns:q='u' p:x='1' q:x='2'/>", true, ErrorKind::DuplicateAttribute),
    ("colon in pi target", b"<a><?a:b x?></a>", true, ErrorKind::InvalidToken),
];

/// Cases where expat-rs deliberately reports a different kind.
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[];

fn kind_of(input: &[u8], namespaces: bool) -> Option<ErrorKind> {
    let result = (|| -> Result<(), XmlError> {
        let text = decode(input)?;
        let mut p = Parser::new(&text);
        if namespaces { p = p.with_namespaces(); }
        while p.next_event()?.is_some() {}
        Ok(())
    })();
    result.err().map(|e| e.kind())
}

#[test]
fn error_kinds_match_libexpat() {
    let mut mismatches = Vec::new();
    for &(name, input, ns, want) in CASES {
        if KNOWN_DIVERGENCES.iter().any(|(n, _)| *n == name) { continue; }
        let got = kind_of(input, ns);
        if got != Some(want) {
            mismatches.push(format!("{name}: libexpat {want:?}, expat-rs {got:?}"));
        }
    }
    assert!(mismatches.is_empty(), "{} mismatches:\n{}", mismatches.len(), mismatches.join("\n"));
}
