use pain::lexer::{TokenKind, tokenize};

#[test]
fn tokenizes_declarations_and_annotations() {
    let src = "schema test.v1;\nmessage Ping @tag(0xFE) {\n  id: guid @sentinel(empty);\n}\n";
    let tokens = tokenize(src).unwrap();
    assert_eq!(tokens[0].kind, TokenKind::Ident("schema".into()));
    assert_eq!(tokens[1].kind, TokenKind::Ident("test".into()));
    assert_eq!(tokens[2].kind, TokenKind::Dot);
    assert_eq!(tokens[3].kind, TokenKind::Ident("v1".into()));
    assert_eq!(tokens[4].kind, TokenKind::Semi);
    assert_eq!(tokens[5].kind, TokenKind::Ident("message".into()));
    assert_eq!(tokens[6].kind, TokenKind::Ident("Ping".into()));
    assert_eq!(tokens[7].kind, TokenKind::At);
    assert_eq!(tokens[8].kind, TokenKind::Ident("tag".into()));
    assert_eq!(tokens[9].kind, TokenKind::LParen);
    assert_eq!(tokens[10].kind, TokenKind::Number(0xFE));
    assert_eq!(tokens[11].kind, TokenKind::RParen);
}

#[test]
fn rejects_unterminated_string() {
    let err = tokenize("struct Bad { s: string @sentinel(\"unclosed); }").unwrap_err();
    assert_eq!(err.line, 1);
    assert!(err.message.contains("unterminated string"));
}
