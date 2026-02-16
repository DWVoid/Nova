use crate::token::{Comment, CommentKind, Keyword, Position, Span, Symbol, Token, TokenKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexError {
    pub message: String,
    pub position: Position,
}

pub struct Lexer<'a> {
    input: &'a str,
    index: usize,
    position: Position,
    pending_leading: Vec<Comment>,
    newline_since_token: bool,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            index: 0,
            position: Position::start(),
            pending_leading: Vec::new(),
            newline_since_token: true,
            tokens: Vec::new(),
        }
    }

    pub fn lex_all(mut self) -> Result<Vec<Token>, LexError> {
        while !self.is_eof() {
            self.skip_whitespace_and_comments()?;
            if self.is_eof() {
                break;
            }
            let token = self.lex_token()?;
            self.tokens.push(token);
            self.newline_since_token = false;
        }

        let eof_span = Span::single(self.position);
        let mut eof = Token::new(TokenKind::Eof, eof_span);
        eof.leading.append(&mut self.pending_leading);
        self.tokens.push(eof);
        Ok(self.tokens)
    }

    fn is_eof(&self) -> bool {
        self.index >= self.input.len()
    }

    fn current_slice(&self) -> &'a str {
        &self.input[self.index..]
    }

    fn peek_char(&self) -> Option<char> {
        self.current_slice().chars().next()
    }

    fn peek_char_n(&self, n: usize) -> Option<char> {
        self.current_slice().chars().nth(n)
    }

    fn consume_str(&mut self, text: &str) {
        self.index += text.len();
        self.position = self.position.advance(text);
    }

    fn consume_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        let len = ch.len_utf8();
        let text = &self.input[self.index..self.index + len];
        self.consume_str(text);
        Some(ch)
    }

    fn consume_newline(&mut self) -> bool {
        if self.current_slice().starts_with("\r\n") {
            self.consume_str("\r\n");
            true
        } else if self.current_slice().starts_with('\n') {
            self.consume_str("\n");
            true
        } else if self.current_slice().starts_with('\r') {
            self.consume_str("\r");
            true
        } else {
            false
        }
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), LexError> {
        loop {
            if self.consume_newline() {
                self.newline_since_token = true;
                continue;
            }

            let Some(ch) = self.peek_char() else { break };
            if ch == ' ' || ch == '\t' || ch == '\u{000B}' || ch == '\u{000C}' {
                self.consume_char();
                continue;
            }

            if self.current_slice().starts_with("--") {
                self.lex_comment()?;
                continue;
            }

            break;
        }
        Ok(())
    }

    fn lex_comment(&mut self) -> Result<(), LexError> {
        let start_pos = self.position;
        let start_index = self.index;
        self.consume_str("--");

        if let Some(level) = self.peek_long_bracket_level() {
            self.consume_long_bracket(level, CommentKind::Block, start_pos, start_index)?;
            return Ok(());
        }

        while !self.is_eof() && !self.current_slice().starts_with('\n') && !self.current_slice().starts_with('\r') {
            self.consume_char();
        }

        let end_index = self.index;
        let text = self.input[start_index..end_index].to_string();
        let span = Span::new(start_pos, self.position);
        let comment = Comment {
            kind: CommentKind::Line,
            text,
            span,
        };
        self.attach_comment(comment);
        Ok(())
    }

    fn attach_comment(&mut self, comment: Comment) {
        if !self.newline_since_token {
            if let Some(last) = self.tokens.last_mut() {
                last.trailing.push(comment);
                return;
            }
        }
        self.pending_leading.push(comment);
    }

    fn lex_token(&mut self) -> Result<Token, LexError> {
        let start_pos = self.position;
        let token = match self.peek_char() {
            Some(ch) if is_ident_start(ch) => self.lex_identifier_or_keyword(start_pos)?,
            Some(ch) if ch.is_ascii_digit() => self.lex_number(start_pos)?,
            Some('.') if self.peek_char_n(1).map_or(false, |c| c.is_ascii_digit()) => {
                self.lex_number(start_pos)?
            }
            Some('"') | Some('\'') => self.lex_short_string(start_pos)?,
            Some('[') => {
                if let Some(level) = self.peek_long_bracket_level() {
                    self.lex_long_string(start_pos, level)?
                } else {
                    self.lex_symbol(start_pos)?
                }
            }
            Some(_) => self.lex_symbol(start_pos)?,
            None => {
                return Err(LexError {
                    message: "unexpected EOF".to_string(),
                    position: self.position,
                })
            }
        };

        let mut token = token;
        token.leading.append(&mut self.pending_leading);
        Ok(token)
    }

    fn lex_identifier_or_keyword(&mut self, start_pos: Position) -> Result<Token, LexError> {
        let start_index = self.index;
        self.consume_char();
        while let Some(ch) = self.peek_char() {
            if is_ident_continue(ch) {
                self.consume_char();
            } else {
                break;
            }
        }
        let text = &self.input[start_index..self.index];
        let kind = if let Some(keyword) = keyword_from_str(text) {
            TokenKind::Keyword(keyword)
        } else {
            TokenKind::Identifier(text.to_string())
        };
        Ok(Token::new(kind, Span::new(start_pos, self.position)))
    }

    fn lex_number(&mut self, start_pos: Position) -> Result<Token, LexError> {
        let start_index = self.index;
        if self.current_slice().starts_with("0x") || self.current_slice().starts_with("0X") {
            self.consume_str("0x");
            self.consume_hex_digits();
            if self.peek_char() == Some('.') {
                self.consume_char();
                self.consume_hex_digits();
            }
            if matches!(self.peek_char(), Some('p') | Some('P')) {
                self.consume_char();
                if matches!(self.peek_char(), Some('+') | Some('-')) {
                    self.consume_char();
                }
                self.consume_dec_digits();
            }
        } else {
            self.consume_dec_digits();
            if self.peek_char() == Some('.') {
                self.consume_char();
                self.consume_dec_digits();
            }
            if matches!(self.peek_char(), Some('e') | Some('E')) {
                self.consume_char();
                if matches!(self.peek_char(), Some('+') | Some('-')) {
                    self.consume_char();
                }
                self.consume_dec_digits();
            }
        }
        let text = self.input[start_index..self.index].to_string();
        Ok(Token::new(
            TokenKind::Number(text),
            Span::new(start_pos, self.position),
        ))
    }

    fn consume_dec_digits(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.consume_char();
            } else {
                break;
            }
        }
    }

    fn consume_hex_digits(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_hexdigit() {
                self.consume_char();
            } else {
                break;
            }
        }
    }

    fn lex_short_string(&mut self, start_pos: Position) -> Result<Token, LexError> {
        let quote = self.consume_char().ok_or(LexError {
            message: "unexpected EOF in string".to_string(),
            position: self.position,
        })?;
        let mut content = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == quote {
                self.consume_char();
                let span = Span::new(start_pos, self.position);
                return Ok(Token::new(TokenKind::StringLiteral(content), span));
            }

            if ch == '\\' {
                self.consume_char();
                let Some(escaped) = self.consume_char() else {
                    return Err(LexError {
                        message: "unterminated escape sequence".to_string(),
                        position: self.position,
                    });
                };
                content.push('\\');
                content.push(escaped);
                continue;
            }

            if ch == '\n' || ch == '\r' {
                return Err(LexError {
                    message: "newline in short string".to_string(),
                    position: self.position,
                });
            }

            self.consume_char();
            content.push(ch);
        }

        Err(LexError {
            message: "unterminated string literal".to_string(),
            position: self.position,
        })
    }

    fn peek_long_bracket_level(&self) -> Option<usize> {
        if !self.current_slice().starts_with('[') {
            return None;
        }
        let mut level = 0;
        let mut idx = self.index + 1;
        let bytes = self.input.as_bytes();
        while idx < bytes.len() && bytes[idx] == b'=' {
            level += 1;
            idx += 1;
        }
        if idx < bytes.len() && bytes[idx] == b'[' {
            Some(level)
        } else {
            None
        }
    }

    fn consume_long_bracket(
        &mut self,
        level: usize,
        kind: CommentKind,
        start_pos: Position,
        start_index: usize,
    ) -> Result<(), LexError> {
        let open = format!("[{}[", "=".repeat(level));
        self.consume_str(&open);

        if self.consume_newline() {
            // Skip the first newline in long strings/comments.
        }

        while !self.is_eof() {
            if self.current_slice().starts_with(']') {
                let mut idx = self.index + 1;
                let bytes = self.input.as_bytes();
                let mut seen = 0;
                while idx < bytes.len() && bytes[idx] == b'=' {
                    seen += 1;
                    idx += 1;
                }
                if seen == level && idx < bytes.len() && bytes[idx] == b']' {
                    let close = format!("]{}]", "=".repeat(level));
                    self.consume_str(&close);
                    let end_index = self.index;
                    let text = self.input[start_index..end_index].to_string();
                    let span = Span::new(start_pos, self.position);
                    let comment = Comment { kind, text, span };
                    self.attach_comment(comment);
                    return Ok(());
                }
            }

            if self.consume_newline() {
                continue;
            }
            self.consume_char();
        }

        Err(LexError {
            message: "unterminated long comment".to_string(),
            position: self.position,
        })
    }

    fn lex_long_string(&mut self, start_pos: Position, level: usize) -> Result<Token, LexError> {
        let open = format!("[{}[", "=".repeat(level));
        self.consume_str(&open);

        if self.consume_newline() {
            // Skip the first newline in long strings.
        }

        let mut content = String::new();
        while !self.is_eof() {
            if self.current_slice().starts_with(']') {
                let mut idx = self.index + 1;
                let bytes = self.input.as_bytes();
                let mut seen = 0;
                while idx < bytes.len() && bytes[idx] == b'=' {
                    seen += 1;
                    idx += 1;
                }
                if seen == level && idx < bytes.len() && bytes[idx] == b']' {
                    let close = format!("]{}]", "=".repeat(level));
                    self.consume_str(&close);
                    let span = Span::new(start_pos, self.position);
                    return Ok(Token::new(TokenKind::StringLiteral(content), span));
                }
            }

            if self.consume_newline() {
                content.push('\n');
                continue;
            }
            let ch = self.consume_char().ok_or(LexError {
                message: "unterminated long string".to_string(),
                position: self.position,
            })?;
            content.push(ch);
        }

        Err(LexError {
            message: "unterminated long string".to_string(),
            position: self.position,
        })
    }

    fn lex_symbol(&mut self, start_pos: Position) -> Result<Token, LexError> {
        let slice = self.current_slice();
        let (kind, consume) = if slice.starts_with("..") {
            (TokenKind::Symbol(Symbol::DotDot), "..")
        } else if slice.starts_with("==") {
            (TokenKind::Symbol(Symbol::EqEq), "==")
        } else if slice.starts_with("~=") {
            (TokenKind::Symbol(Symbol::NotEq), "~=")
        } else if slice.starts_with("<=") {
            (TokenKind::Symbol(Symbol::LessEq), "<=")
        } else if slice.starts_with(">=") {
            (TokenKind::Symbol(Symbol::GreaterEq), ">=")
        } else if slice.starts_with("<<") {
            (TokenKind::Symbol(Symbol::ShiftLeft), "<<")
        } else if slice.starts_with(">>") {
            (TokenKind::Symbol(Symbol::ShiftRight), ">>")
        } else if slice.starts_with("//") {
            (TokenKind::Symbol(Symbol::FloorDiv), "//")
        } else {
            let ch = self.peek_char().ok_or(LexError {
                message: "unexpected EOF".to_string(),
                position: self.position,
            })?;
            let sym = match ch {
                '+' => Symbol::Plus,
                '-' => Symbol::Minus,
                '*' => Symbol::Star,
                '/' => Symbol::Slash,
                '%' => Symbol::Percent,
                '^' => Symbol::Caret,
                '#' => Symbol::Hash,
                '&' => Symbol::Amp,
                '~' => Symbol::Tilde,
                '|' => Symbol::Pipe,
                '<' => Symbol::Less,
                '>' => Symbol::Greater,
                '=' => Symbol::Assign,
                '(' => Symbol::LParen,
                ')' => Symbol::RParen,
                '{' => Symbol::LBrace,
                '}' => Symbol::RBrace,
                '[' => Symbol::LBracket,
                ']' => Symbol::RBracket,
                ';' => Symbol::Semi,
                ':' => Symbol::Colon,
                ',' => Symbol::Comma,
                '.' => Symbol::Dot,
                '@' => Symbol::At,
                _ => {
                    return Err(LexError {
                        message: format!("unexpected character: {ch}"),
                        position: self.position,
                    })
                }
            };
            let len = ch.len_utf8();
            let text = &self.input[self.index..self.index + len];
            (TokenKind::Symbol(sym), text)
        };

        self.consume_str(consume);
        Ok(Token::new(kind, Span::new(start_pos, self.position)))
    }
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn keyword_from_str(text: &str) -> Option<Keyword> {
    Some(match text {
        "and" => Keyword::And,
        "as" => Keyword::As,
        "break" => Keyword::Break,
        "const" => Keyword::Const,
        "continue" => Keyword::Continue,
        "define" => Keyword::Define,
        "do" => Keyword::Do,
        "else" => Keyword::Else,
        "elseif" => Keyword::ElseIf,
        "end" => Keyword::End,
        "enum" => Keyword::Enum,
        "export" => Keyword::Export,
        "false" => Keyword::False,
        "for" => Keyword::For,
        "goto" => Keyword::Goto,
        "if" => Keyword::If,
        "implement" => Keyword::Implement,
        "in" => Keyword::In,
        "namespace" => Keyword::Namespace,
        "nil" => Keyword::Nil,
        "not" => Keyword::Not,
        "or" => Keyword::Or,
        "repeat" => Keyword::Repeat,
        "return" => Keyword::Return,
        "struct" => Keyword::Struct,
        "then" => Keyword::Then,
        "trait" => Keyword::Trait,
        "true" => Keyword::True,
        "until" => Keyword::Until,
        "use" => Keyword::Use,
        "val" => Keyword::Val,
        "var" => Keyword::Var,
        "variant" => Keyword::Variant,
        "while" => Keyword::While,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_simple_tokens_with_comment() {
        let input = "-- hi\nuse System;";
        let tokens = Lexer::new(input).lex_all().unwrap();
        let first = &tokens[0];
        assert!(matches!(first.kind, TokenKind::Keyword(Keyword::Use)));
        assert_eq!(first.leading.len(), 1);
        assert!(matches!(tokens[1].kind, TokenKind::Identifier(_)));
        assert!(matches!(tokens[2].kind, TokenKind::Symbol(Symbol::Semi)));
    }

    #[test]
    fn lexes_long_string() {
        let input = "[[a\nb]]";
        let tokens = Lexer::new(input).lex_all().unwrap();
        let first = &tokens[0];
        match &first.kind {
            TokenKind::StringLiteral(text) => assert_eq!(text, "a\nb"),
            _ => panic!("expected long string"),
        }
    }

    #[test]
    fn lexes_numbers() {
        let input = "12 0x1.2p3 3.14";
        let tokens = Lexer::new(input).lex_all().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Number(_)));
        assert!(matches!(tokens[1].kind, TokenKind::Number(_)));
        assert!(matches!(tokens[2].kind, TokenKind::Number(_)));
    }
}
