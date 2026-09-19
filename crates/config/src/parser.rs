use crate::ast::{Arg, Block, Directive, Document, Item};
use crate::error::{ConfigError, ErrorCode};
use crate::lexer::{Lexer, TokenKind};

pub fn parse(src: &str, file: &str) -> Result<Document, Vec<ConfigError>> {
    let mut lx = Lexer::new(src);
    let mut errors = Vec::new();
    let mut items = Vec::new();
    loop {
        match parse_item(&mut lx, file, &mut errors) {
            Ok(Some(item)) => items.push(item),
            Ok(None) => break,
            Err(()) => {
                // recover: skip to next ; or }
                recover(&mut lx);
            }
        }
    }
    if errors.is_empty() {
        Ok(Document { items })
    } else {
        Err(errors)
    }
}

fn recover(lx: &mut Lexer<'_>) {
    loop {
        match lx.next_token() {
            Ok(t) if matches!(t.kind, TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof) => {
                break
            }
            Ok(_) => {}
            Err(_) => {
                // Advance one byte so unexpected characters cannot spin forever.
                let _ = lx.force_bump();
                break;
            }
        }
    }
}

fn parse_item(
    lx: &mut Lexer<'_>,
    file: &str,
    errors: &mut Vec<ConfigError>,
) -> Result<Option<Item>, ()> {
    let tok = match lx.next_token() {
        Ok(t) => t,
        Err((line, col, msg)) => {
            errors.push(ConfigError::new(
                file,
                line,
                col,
                "",
                ErrorCode::Syntax,
                msg,
            ));
            return Err(());
        }
    };
    match tok.kind {
        TokenKind::Eof => Ok(None),
        TokenKind::Ident(name) => {
            let mut args = Vec::new();
            let name_line = tok.line;
            let name_col = tok.col;
            loop {
                let peek = match lx.next_token() {
                    Ok(t) => t,
                    Err((line, col, msg)) => {
                        errors.push(ConfigError::new(
                            file,
                            line,
                            col,
                            &name,
                            ErrorCode::Syntax,
                            msg,
                        ));
                        return Err(());
                    }
                };
                match peek.kind {
                    TokenKind::Semi => {
                        return Ok(Some(Item::Directive(Directive {
                            name,
                            args,
                            line: name_line,
                            col: name_col,
                        })));
                    }
                    TokenKind::LBrace => {
                        let items = parse_block_body(lx, file, errors)?;
                        return Ok(Some(Item::Block(Block {
                            name,
                            args,
                            items,
                            line: name_line,
                            col: name_col,
                        })));
                    }
                    TokenKind::Ident(s) => args.push(Arg::Ident(s)),
                    TokenKind::Number(n) => args.push(Arg::Number(n)),
                    TokenKind::Size(n) => args.push(Arg::Size(n)),
                    TokenKind::DurationMs(n) => args.push(Arg::DurationMs(n)),
                    TokenKind::String(s) => args.push(Arg::String(s)),
                    TokenKind::Path(s) => args.push(Arg::Path(s)),
                    TokenKind::RBrace | TokenKind::Eof => {
                        errors.push(ConfigError::new(
                            file,
                            peek.line,
                            peek.col,
                            &name,
                            ErrorCode::Syntax,
                            format!("expected ';' or '{{' after '{name}'"),
                        ));
                        return Err(());
                    }
                }
            }
        }
        other => {
            errors.push(ConfigError::new(
                file,
                tok.line,
                tok.col,
                "",
                ErrorCode::Syntax,
                format!("expected directive, got {other:?}"),
            ));
            Err(())
        }
    }
}

fn parse_block_body(
    lx: &mut Lexer<'_>,
    file: &str,
    errors: &mut Vec<ConfigError>,
) -> Result<Vec<Item>, ()> {
    let mut items = Vec::new();
    loop {
        // peek by parsing — need look-ahead for }
        // Re-read: call next; if RBrace done; else push back... Lexer has no pushback.
        // Parse using a one-token lookahead buffer would be better.
        // Simpler approach: parse_item but detect RBrace first via temporary.
        let tok = match lx.next_token() {
            Ok(t) => t,
            Err((line, col, msg)) => {
                errors.push(ConfigError::new(
                    file,
                    line,
                    col,
                    "",
                    ErrorCode::Syntax,
                    msg,
                ));
                return Err(());
            }
        };
        match tok.kind {
            TokenKind::RBrace => return Ok(items),
            TokenKind::Eof => {
                errors.push(ConfigError::new(
                    file,
                    tok.line,
                    tok.col,
                    "",
                    ErrorCode::Syntax,
                    "unexpected end of file inside block",
                ));
                return Err(());
            }
            TokenKind::Ident(name) => {
                let mut args = Vec::new();
                let name_line = tok.line;
                let name_col = tok.col;
                loop {
                    let peek = match lx.next_token() {
                        Ok(t) => t,
                        Err((line, col, msg)) => {
                            errors.push(ConfigError::new(
                                file,
                                line,
                                col,
                                &name,
                                ErrorCode::Syntax,
                                msg,
                            ));
                            return Err(());
                        }
                    };
                    match peek.kind {
                        TokenKind::Semi => {
                            items.push(Item::Directive(Directive {
                                name,
                                args,
                                line: name_line,
                                col: name_col,
                            }));
                            break;
                        }
                        TokenKind::LBrace => {
                            let nested = parse_block_body(lx, file, errors)?;
                            items.push(Item::Block(Block {
                                name,
                                args,
                                items: nested,
                                line: name_line,
                                col: name_col,
                            }));
                            break;
                        }
                        TokenKind::Ident(s) => args.push(Arg::Ident(s)),
                        TokenKind::Number(n) => args.push(Arg::Number(n)),
                        TokenKind::Size(n) => args.push(Arg::Size(n)),
                        TokenKind::DurationMs(n) => args.push(Arg::DurationMs(n)),
                        TokenKind::String(s) => args.push(Arg::String(s)),
                        TokenKind::Path(s) => args.push(Arg::Path(s)),
                        TokenKind::RBrace | TokenKind::Eof => {
                            errors.push(ConfigError::new(
                                file,
                                peek.line,
                                peek.col,
                                &name,
                                ErrorCode::Syntax,
                                format!("expected ';' or '{{' after '{name}'"),
                            ));
                            return Err(());
                        }
                    }
                }
            }
            other => {
                errors.push(ConfigError::new(
                    file,
                    tok.line,
                    tok.col,
                    "",
                    ErrorCode::Syntax,
                    format!("expected directive inside block, got {other:?}"),
                ));
                return Err(());
            }
        }
    }
}
