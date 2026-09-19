//! Nginx-style lexer.

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(u64),
    Size(u64),
    DurationMs(u64),
    String(String),
    Path(String),
    LBrace,
    RBrace,
    Semi,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: u32,
    pub col: u32,
}

pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    i: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            i: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn next_token(&mut self) -> Result<Token, (u32, u32, String)> {
        self.skip_ws_comments();
        let line = self.line;
        let col = self.col;
        if self.i >= self.bytes.len() {
            return Ok(Token {
                kind: TokenKind::Eof,
                line,
                col,
            });
        }
        let c = self.bytes[self.i] as char;
        match c {
            '{' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::LBrace,
                    line,
                    col,
                })
            }
            '}' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::RBrace,
                    line,
                    col,
                })
            }
            ';' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Semi,
                    line,
                    col,
                })
            }
            '"' => self.lex_string(line, col),
            _ if c.is_ascii_digit() => self.lex_dotted_or_number(line, col),
            _ if c.is_ascii_alphabetic() || c == '_' || c == '/' => self.lex_ident_or_path(line, col),
            _ => Err((line, col, format!("unexpected character {c:?}"))),
        }
    }

    fn bump(&mut self) {
        if self.i < self.bytes.len() {
            if self.bytes[self.i] == b'\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.i += 1;
        }
    }

    /// Public bump for parser recovery.
    pub fn force_bump(&mut self) {
        self.bump();
    }

    fn skip_ws_comments(&mut self) {
        loop {
            while self.i < self.bytes.len()
                && matches!(self.bytes[self.i], b' ' | b'\t' | b'\r' | b'\n')
            {
                self.bump();
            }
            if self.i < self.bytes.len() && self.bytes[self.i] == b'#' {
                while self.i < self.bytes.len() && self.bytes[self.i] != b'\n' {
                    self.bump();
                }
                continue;
            }
            break;
        }
    }

    fn lex_string(&mut self, line: u32, col: u32) -> Result<Token, (u32, u32, String)> {
        self.bump(); // "
        let mut out = String::new();
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i] as char;
            if c == '"' {
                self.bump();
                return Ok(Token {
                    kind: TokenKind::String(out),
                    line,
                    col,
                });
            }
            if c == '\\' {
                self.bump();
                if self.i >= self.bytes.len() {
                    return Err((line, col, "unterminated string escape".into()));
                }
                out.push(self.bytes[self.i] as char);
                self.bump();
                continue;
            }
            out.push(c);
            self.bump();
        }
        Err((line, col, "unterminated string".into()))
    }

    fn lex_numberish(&mut self, line: u32, col: u32) -> Result<Token, (u32, u32, String)> {
        let start = self.i;
        while self.i < self.bytes.len() && self.bytes[self.i].is_ascii_digit() {
            self.bump();
        }
        let num: u64 = self.src[start..self.i]
            .parse()
            .map_err(|_| (line, col, "bad number".into()))?;
        if self.i >= self.bytes.len() {
            return Ok(Token {
                kind: TokenKind::Number(num),
                line,
                col,
            });
        }
        let rest = &self.src[self.i..];
        if rest.starts_with("ms") {
            self.bump();
            self.bump();
            return Ok(Token {
                kind: TokenKind::DurationMs(num),
                line,
                col,
            });
        }
        let ch = rest.chars().next().unwrap();
        match ch {
            's' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::DurationMs(num.saturating_mul(1000)),
                    line,
                    col,
                })
            }
            'h' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::DurationMs(num.saturating_mul(3_600_000)),
                    line,
                    col,
                })
            }
            // SIZE units (MiB etc.). Duration minutes also use 'm' in the grammar;
            // first-binary configs use `Ns` for drain_timeout and `Nm` for buffers.
            'k' | 'K' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Size(num.saturating_mul(1024)),
                    line,
                    col,
                })
            }
            'm' | 'M' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Size(num.saturating_mul(1024 * 1024)),
                    line,
                    col,
                })
            }
            'g' | 'G' => {
                self.bump();
                Ok(Token {
                    kind: TokenKind::Size(num.saturating_mul(1024 * 1024 * 1024)),
                    line,
                    col,
                })
            }
            _ => Ok(Token {
                kind: TokenKind::Number(num),
                line,
                col,
            }),
        }
    }

    fn lex_ident_or_path(&mut self, line: u32, col: u32) -> Result<Token, (u32, u32, String)> {
        let start = self.i;
        while self.i < self.bytes.len() {
            let c = self.bytes[self.i] as char;
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '~') {
                self.bump();
            } else {
                break;
            }
        }
        let s = self.src[start..self.i].to_string();
        if s.contains('/') || s.contains(':') {
            Ok(Token {
                kind: TokenKind::Path(s),
                line,
                col,
            })
        } else {
            Ok(Token {
                kind: TokenKind::Ident(s),
                line,
                col,
            })
        }
    }

    /// Lex IPv4 / dotted hosts that start with a digit (e.g. 0.0.0.0, 127.0.0.1).
    fn lex_dotted_or_number(&mut self, line: u32, col: u32) -> Result<Token, (u32, u32, String)> {
        let start = self.i;
        // Look ahead: if digits and dots only (and possibly more), treat as host ident.
        let mut j = self.i;
        let mut saw_dot = false;
        while j < self.bytes.len() {
            let c = self.bytes[j] as char;
            if c.is_ascii_digit() {
                j += 1;
            } else if c == '.' {
                saw_dot = true;
                j += 1;
            } else {
                break;
            }
        }
        if saw_dot {
            while self.i < j {
                self.bump();
            }
            let s = self.src[start..self.i].to_string();
            return Ok(Token {
                kind: TokenKind::Ident(s),
                line,
                col,
            });
        }
        self.lex_numberish(line, col)
    }
}
