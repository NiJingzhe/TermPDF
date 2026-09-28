use termpdf::reference::DocumentRef;

#[test]
fn parses_and_formats_supported_document_refs() {
    let cases = [
        ("p1", DocumentRef::Page { page: 0 }),
        ("p2.t3", DocumentRef::TextLine { page: 1, line: 2 }),
        (
            "p4.t5.c6",
            DocumentRef::Glyph {
                page: 3,
                line: 4,
                glyph: 5,
            },
        ),
        ("p7.link8", DocumentRef::Link { page: 6, link: 7 }),
        ("p9.image10", DocumentRef::Image { page: 8, image: 9 }),
    ];

    for (input, expected) in cases {
        let parsed = input.parse::<DocumentRef>().unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(parsed.to_string(), input);
    }
}

#[test]
fn rejects_zero_based_malformed_and_unknown_document_refs() {
    for input in [
        "",
        "p0",
        "p1.t0",
        "p1.t1.c0",
        "1",
        "p1.foo1",
        "p1.t1.extra1",
        "p1.image",
    ] {
        assert!(input.parse::<DocumentRef>().is_err(), "accepted {input}");
    }
}
