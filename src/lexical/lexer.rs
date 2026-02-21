use crate::lexical::token::{
    Comment, CommentKind, Keyword, Position, Span, Symbol, Token, TokenKind,
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexError {
    pub message: String,
    pub position: Position,
}

/// The result of lexing a source file: a clean token stream and a separate,
/// ordered list of every comment found in the source.
#[derive(Clone, Debug)]
pub struct LexResult {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
}

/// Lex a Nova source string into a token stream and a flat comment list.
pub fn lex(input: &str) -> Result<LexResult, LexError> {
    Lexer::new(input).scan()
}

struct Lexer<'a> {
    input: &'a str,
    index: usize,
    position: Position,
    comments: Vec<Comment>,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            index: 0,
            position: Position::new_start(),
            comments: Vec::new(),
            tokens: Vec::new(),
        }
    }

    fn scan(mut self) -> Result<LexResult, LexError> {
        while !self.is_eof() {
            self.advance_trivia()?;
            if self.is_eof() {
                break;
            }
            let token = self.scan_token()?;
            self.tokens.push(token);
        }

        let eof_span = Span::single(self.position);
        self.tokens.push(Token::new(TokenKind::Eof, eof_span));
        Ok(LexResult {
            tokens: self.tokens,
            comments: self.comments,
        })
    }

    fn is_eof(&self) -> bool {
        self.index >= self.input.len()
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.index..]
    }

    fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn peek_char_at(&self, n: usize) -> Option<char> {
        self.remaining().chars().nth(n)
    }

    fn advance_by(&mut self, text: &str) {
        self.index += text.len();
        self.position = self.advance_position(self.position, text);
    }

    /// Recomputes a `Position` after consuming `text`, tracking bytes, grapheme
    /// clusters, lines, and columns.  Lives here rather than on `Position` itself
    /// because it is the only place position arithmetic is needed.
    fn advance_position(&self, pos: Position, text: &str) -> Position {
        let byte = pos.byte() + text.len();
        let mut grapheme = pos.grapheme();
        let mut line = pos.line();
        let mut column = pos.column();
        let mut idx = 0;
        while idx < text.len() {
            let slice = &text[idx..];
            if slice.starts_with("\r\n") {
                grapheme += 1;
                line += 1;
                column = 0;
                idx += 2;
                continue;
            }
            if slice.starts_with('\n') || slice.starts_with('\r') {
                grapheme += 1;
                line += 1;
                column = 0;
                idx += 1;
                continue;
            }
            let g = UnicodeSegmentation::graphemes(slice, true).next().unwrap();
            grapheme += 1;
            column += 1;
            idx += g.len();
        }
        Position::new(byte, grapheme, line, column)
    }

    fn advance_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        let len = ch.len_utf8();
        let text = &self.input[self.index..self.index + len];
        self.advance_by(text);
        Some(ch)
    }

    fn advance_newline(&mut self) -> bool {
        if self.remaining().starts_with("\r\n") {
            self.advance_by("\r\n");
            true
        } else if self.remaining().starts_with('\n') {
            self.advance_by("\n");
            true
        } else if self.remaining().starts_with('\r') {
            self.advance_by("\r");
            true
        } else {
            false
        }
    }

    fn advance_trivia(&mut self) -> Result<(), LexError> {
        loop {
            if self.advance_newline() {
                continue;
            }

            let Some(ch) = self.peek_char() else { break };
            if ch == ' ' || ch == '\t' || ch == '\u{000B}' || ch == '\u{000C}' {
                self.advance_char();
                continue;
            }

            if self.remaining().starts_with("--") {
                self.scan_comment()?;
                continue;
            }

            break;
        }
        Ok(())
    }

    fn scan_comment(&mut self) -> Result<(), LexError> {
        let start_pos = self.position;
        let start_index = self.index;
        self.advance_by("--");

        if let Some(level) = self.peek_long_bracket_level() {
            self.scan_long_bracket_comment(level, start_pos, start_index)?;
            return Ok(());
        }

        while !self.is_eof()
            && !self.remaining().starts_with('\n')
            && !self.remaining().starts_with('\r')
        {
            self.advance_char();
        }

        let end_index = self.index;
        let text = self.input[start_index..end_index].to_string();
        let span = Span::new(start_pos, self.position);
        self.comments.push(Comment {
            kind: CommentKind::Line,
            text,
            span,
        });
        Ok(())
    }

    fn scan_token(&mut self) -> Result<Token, LexError> {
        let start_pos = self.position;
        let token = match self.peek_char() {
            Some(ch) if is_ident_start(ch) => self.scan_word(start_pos)?,
            Some(ch) if ch.is_ascii_digit() => self.scan_number(start_pos)?,
            Some('.') if self.peek_char_at(1).map_or(false, |c| c.is_ascii_digit()) => {
                self.scan_number(start_pos)?
            }
            Some('"') | Some('\'') => self.scan_short_string(start_pos)?,
            Some('[') => {
                if let Some(level) = self.peek_long_bracket_level() {
                    self.scan_long_string(start_pos, level)?
                } else {
                    self.scan_symbol(start_pos)?
                }
            }
            Some(_) => self.scan_symbol(start_pos)?,
            None => {
                return Err(LexError {
                    message: "unexpected EOF".to_string(),
                    position: self.position,
                });
            }
        };

        Ok(token)
    }

    fn scan_word(&mut self, start_pos: Position) -> Result<Token, LexError> {
        let start_index = self.index;
        self.advance_char();
        while let Some(ch) = self.peek_char() {
            if is_ident_continue(ch) {
                self.advance_char();
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

    fn scan_number(&mut self, start_pos: Position) -> Result<Token, LexError> {
        let start_index = self.index;
        if self.remaining().starts_with("0x") || self.remaining().starts_with("0X") {
            self.advance_by("0x");
            self.advance_hex_digits();
            if self.peek_char() == Some('.') {
                self.advance_char();
                self.advance_hex_digits();
            }
            if matches!(self.peek_char(), Some('p') | Some('P')) {
                self.advance_char();
                if matches!(self.peek_char(), Some('+') | Some('-')) {
                    self.advance_char();
                }
                self.advance_dec_digits();
            }
        } else {
            self.advance_dec_digits();
            if self.peek_char() == Some('.') {
                self.advance_char();
                self.advance_dec_digits();
            }
            if matches!(self.peek_char(), Some('e') | Some('E')) {
                self.advance_char();
                if matches!(self.peek_char(), Some('+') | Some('-')) {
                    self.advance_char();
                }
                self.advance_dec_digits();
            }
        }
        let text = self.input[start_index..self.index].to_string();
        Ok(Token::new(
            TokenKind::Number(text),
            Span::new(start_pos, self.position),
        ))
    }

    fn advance_dec_digits(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.advance_char();
            } else {
                break;
            }
        }
    }

    fn advance_hex_digits(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_hexdigit() {
                self.advance_char();
            } else {
                break;
            }
        }
    }

    fn scan_short_string(&mut self, start_pos: Position) -> Result<Token, LexError> {
        let quote = self.advance_char().ok_or(LexError {
            message: "unexpected EOF in string".to_string(),
            position: self.position,
        })?;
        let mut content = String::new();

        while let Some(ch) = self.peek_char() {
            if ch == quote {
                self.advance_char();
                let span = Span::new(start_pos, self.position);
                return Ok(Token::new(TokenKind::StringLiteral(content), span));
            }

            if ch == '\\' {
                self.advance_char();
                let Some(escaped) = self.advance_char() else {
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

            self.advance_char();
            content.push(ch);
        }

        Err(LexError {
            message: "unterminated string literal".to_string(),
            position: self.position,
        })
    }

    fn peek_long_bracket_level(&self) -> Option<usize> {
        if !self.remaining().starts_with('[') {
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

    // Advances past the opening bracket `[=*[`, skips an optional leading newline,
    // then collects characters until the matching closing bracket `]=*]` is found.
    // Returns the collected content string on success.
    fn scan_long_bracket_body(&mut self, level: usize, what: &str) -> Result<String, LexError> {
        let open = format!("[{}[", "=".repeat(level));
        self.advance_by(&open);

        // ignores the very first newline inside a long bracket.
        self.advance_newline();

        let close = format!("]{}]", "=".repeat(level));
        let mut content = String::new();
        while !self.is_eof() {
            if self.remaining().starts_with(']') {
                let mut idx = self.index + 1;
                let bytes = self.input.as_bytes();
                let mut seen = 0;
                while idx < bytes.len() && bytes[idx] == b'=' {
                    seen += 1;
                    idx += 1;
                }
                if seen == level && idx < bytes.len() && bytes[idx] == b']' {
                    self.advance_by(&close);
                    return Ok(content);
                }
            }

            if self.advance_newline() {
                content.push('\n');
                continue;
            }
            let ch = self.advance_char().ok_or(LexError {
                message: format!("unterminated long {what}"),
                position: self.position,
            })?;
            content.push(ch);
        }

        Err(LexError {
            message: format!("unterminated long {what}"),
            position: self.position,
        })
    }

    fn scan_long_bracket_comment(
        &mut self,
        level: usize,
        start_pos: Position,
        start_index: usize,
    ) -> Result<(), LexError> {
        self.scan_long_bracket_body(level, "comment")?;
        let end_index = self.index;
        let text = self.input[start_index..end_index].to_string();
        let span = Span::new(start_pos, self.position);
        self.comments.push(Comment { kind: CommentKind::Block, text, span });
        Ok(())
    }

    fn scan_long_string(&mut self, start_pos: Position, level: usize) -> Result<Token, LexError> {
        let content = self.scan_long_bracket_body(level, "string")?;
        let span = Span::new(start_pos, self.position);
        Ok(Token::new(TokenKind::StringLiteral(content), span))
    }

    fn scan_symbol(&mut self, start_pos: Position) -> Result<Token, LexError> {
        let slice = self.remaining();
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
                    });
                }
            };
            let len = ch.len_utf8();
            let text = &self.input[self.index..self.index + len];
            (TokenKind::Symbol(sym), text)
        };

        self.advance_by(consume);
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
    fn advance_position_tracks_graphemes_and_lines() {
        let lexer = Lexer::new("");
        let pos = Position::new_start();

        let pos = lexer.advance_position(pos, "a\u{0301}"); // combining accent = 1 grapheme
        assert_eq!(pos.grapheme(), 1);
        assert_eq!(pos.line(), 1);
        assert_eq!(pos.column(), 1);

        let pos = lexer.advance_position(pos, "\n");
        assert_eq!(pos.line(), 2);
        assert_eq!(pos.column(), 0);

        let pos = lexer.advance_position(pos, "\u{03B2}");
        assert_eq!(pos.grapheme(), 3);
        assert_eq!(pos.line(), 2);
        assert_eq!(pos.column(), 1);
    }

    #[test]
    fn lexes_simple_tokens_with_comment() {
        let input = "-- hi\nuse System;";
        let result = lex(input).unwrap();
        assert_eq!(result.comments.len(), 1);
        let first = &result.tokens[0];
        assert!(matches!(first.kind, TokenKind::Keyword(Keyword::Use)));
        assert!(matches!(result.tokens[1].kind, TokenKind::Identifier(_)));
        assert!(matches!(
            result.tokens[2].kind,
            TokenKind::Symbol(Symbol::Semi)
        ));
    }

    #[test]
    fn lexes_long_string() {
        let input = "[[a\nb]]";
        let result = lex(input).unwrap();
        let first = &result.tokens[0];
        match &first.kind {
            TokenKind::StringLiteral(text) => assert_eq!(text, "a\nb"),
            _ => panic!("expected long string"),
        }
    }

    #[test]
    fn lexes_numbers() {
        let input = "12 0x1.2p3 3.14";
        let result = lex(input).unwrap();
        assert!(matches!(result.tokens[0].kind, TokenKind::Number(_)));
        assert!(matches!(result.tokens[1].kind, TokenKind::Number(_)));
        assert!(matches!(result.tokens[2].kind, TokenKind::Number(_)));
    }
}
