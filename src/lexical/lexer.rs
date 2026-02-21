use super::token::{Comment, CommentKind, Keyword, Position, Span, Symbol, Token, TokenKind};
use icu::properties::props::{PatternSyntax, PatternWhiteSpace, XidContinue, XidStart};
use icu::properties::CodePointSetData;
use icu::segmenter::GraphemeClusterSegmenter;

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
            let g_len = {
                let seg = GraphemeClusterSegmenter::new();
                let mut breaks = seg.segment_str(slice);
                breaks.next(); // skip the mandatory break at offset 0
                breaks.next().unwrap_or(slice.len())
            };
            grapheme += 1;
            column += 1;
            idx += g_len;
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

/// Returns true if `ch` may **start** an identifier.
///
/// Follows UAX#31 / C++26: a character is a valid start if it has the
/// `XID_Start` Unicode property, or is U+005F LOW LINE (`_`).
/// Characters with `Pattern_Syntax` or `Pattern_White_Space` are explicitly
/// excluded, which covers all reserved symbols, ASCII punctuation the lexer
/// owns, and all whitespace — so no separate reserved-symbol guard is needed
/// at this layer.
fn is_ident_start(ch: char) -> bool {
    if ch == '_' {
        return true;
    }
    // Pattern_Syntax and Pattern_White_Space are supersets of every ASCII
    // symbol and whitespace character; reject them first so that the lexer's
    // own tokens ('+', '(', '.', etc.) never bleed into identifiers.
    if is_pattern_syntax(ch) || is_pattern_whitespace(ch) {
        return false;
    }
    CodePointSetData::new::<XidStart>().contains(ch)
}

/// Returns true if `ch` may **continue** an identifier (after the first char).
///
/// Follows UAX#31 / C++26: `XID_Continue`, which is a superset of
/// `XID_Start` extended with Mn (non-spacing marks), Mc (spacing combining
/// marks), Nd (decimal digits), and Pc (connector punctuation including `_`).
/// `Pattern_Syntax` and `Pattern_White_Space` are excluded as above.
fn is_ident_continue(ch: char) -> bool {
    if is_pattern_syntax(ch) || is_pattern_whitespace(ch) {
        return false;
    }
    CodePointSetData::new::<XidContinue>().contains(ch)
}

/// Returns true for characters in the Unicode `Pattern_Syntax` property.
/// These are characters reserved for use as syntactic operators in programming
/// languages and are therefore never valid inside an identifier.  The set
/// includes all ASCII punctuation the lexer uses as tokens.
#[inline]
fn is_pattern_syntax(ch: char) -> bool {
    CodePointSetData::new::<PatternSyntax>().contains(ch)
}

/// Returns true for characters in the Unicode `Pattern_White_Space` property.
/// Covers ASCII whitespace (\t, \n, \r, space) plus Unicode line/paragraph
/// separators and a handful of other format whitespace characters.
#[inline]
fn is_pattern_whitespace(ch: char) -> bool {
    CodePointSetData::new::<PatternWhiteSpace>().contains(ch)
}

/// Characters that are always lexed as their own symbol tokens.
/// Kept for use in `scan_symbol`; identifier admission is now handled entirely
/// by `is_pattern_syntax` / `is_pattern_whitespace` above.
#[inline]
fn is_reserved_symbol(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-' | '*' | '/' | '%' | '^' | '#' | '&' | '~' | '|'
        | '<' | '>' | '=' | '(' | ')' | '{' | '}' | '[' | ']'
        | ';' | ':' | ',' | '.' | '@' | '"' | '\''
    )
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

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Lex, assert success, return the token kinds (without the trailing Eof).
    fn tokens(input: &str) -> Vec<TokenKind> {
        let r = lex(input).unwrap_or_else(|e| panic!("lex failed: {}", e.message));
        r.tokens
            .into_iter()
            .filter(|t| !matches!(t.kind, TokenKind::Eof))
            .map(|t| t.kind)
            .collect()
    }

    /// Lex, assert success, return all token kinds including Eof.
    fn tokens_with_eof(input: &str) -> Vec<TokenKind> {
        lex(input)
            .unwrap_or_else(|e| panic!("lex failed: {}", e.message))
            .tokens
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    /// Lex, assert failure, return the error.
    fn must_fail(input: &str) -> LexError {
        lex(input).expect_err("expected lex to fail but it succeeded")
    }

    // ── position tracking ─────────────────────────────────────────────────────

    #[test]
    fn position_starts_at_line_1_column_0() {
        let pos = Position::new_start();
        assert_eq!(pos.byte(), 0);
        assert_eq!(pos.grapheme(), 0);
        assert_eq!(pos.line(), 1);
        assert_eq!(pos.column(), 0);
    }

    #[test]
    fn position_advances_ascii_chars() {
        let lexer = Lexer::new("");
        let pos = lexer.advance_position(Position::new_start(), "abc");
        assert_eq!(pos.byte(), 3);
        assert_eq!(pos.grapheme(), 3);
        assert_eq!(pos.line(), 1);
        assert_eq!(pos.column(), 3);
    }

    #[test]
    fn position_advances_lf_newline() {
        let lexer = Lexer::new("");
        let pos = lexer.advance_position(Position::new_start(), "a\nb");
        assert_eq!(pos.line(), 2);
        assert_eq!(pos.column(), 1);
        assert_eq!(pos.grapheme(), 3);
    }

    #[test]
    fn position_advances_cr_newline() {
        let lexer = Lexer::new("");
        let pos = lexer.advance_position(Position::new_start(), "a\rb");
        assert_eq!(pos.line(), 2);
        assert_eq!(pos.column(), 1);
    }

    #[test]
    fn position_advances_crlf_newline_as_one_grapheme() {
        let lexer = Lexer::new("");
        let pos = lexer.advance_position(Position::new_start(), "a\r\nb");
        assert_eq!(pos.line(), 2);
        assert_eq!(pos.column(), 1);
        // \r\n counts as one grapheme cluster
        assert_eq!(pos.grapheme(), 3);
    }

    #[test]
    fn position_advances_unicode_combining_sequence_as_one_grapheme() {
        let lexer = Lexer::new("");
        // 'a' + combining acute = one grapheme cluster
        let pos = lexer.advance_position(Position::new_start(), "a\u{0301}");
        assert_eq!(pos.grapheme(), 1);
        assert_eq!(pos.column(), 1);
        assert_eq!(pos.byte(), "a\u{0301}".len()); // 3 bytes
    }

    #[test]
    fn position_advances_multibyte_cjk_char() {
        let lexer = Lexer::new("");
        let pos = lexer.advance_position(Position::new_start(), "文");
        assert_eq!(pos.grapheme(), 1);
        assert_eq!(pos.column(), 1);
        assert_eq!(pos.byte(), "文".len()); // 3 bytes
    }

    #[test]
    fn position_byte_offset_is_utf8_byte_count() {
        let lexer = Lexer::new("");
        // emoji is 4 bytes
        let pos = lexer.advance_position(Position::new_start(), "🚀");
        assert_eq!(pos.byte(), 4);
        assert_eq!(pos.grapheme(), 1);
    }

    // ── token spans ───────────────────────────────────────────────────────────

    #[test]
    fn token_span_covers_correct_byte_range() {
        let r = lex("hi").unwrap();
        let tok = &r.tokens[0];
        assert_eq!(tok.span.start.byte(), 0);
        assert_eq!(tok.span.end.byte(), 2);
    }

    #[test]
    fn token_span_reflects_column_position() {
        // "  x" — x starts at column 2
        let r = lex("  x").unwrap();
        assert_eq!(r.tokens[0].span.start.column(), 2);
    }

    #[test]
    fn token_span_on_second_line_has_correct_line() {
        let r = lex("a\nb").unwrap();
        assert_eq!(r.tokens[1].span.start.line(), 2);
        assert_eq!(r.tokens[1].span.start.column(), 0);
    }

    #[test]
    fn eof_token_is_always_last() {
        let kinds = tokens_with_eof("x");
        assert!(matches!(kinds.last(), Some(TokenKind::Eof)));
    }

    #[test]
    fn empty_input_produces_only_eof() {
        let kinds = tokens_with_eof("");
        assert_eq!(kinds.len(), 1);
        assert!(matches!(kinds[0], TokenKind::Eof));
    }

    // ── whitespace and trivia ─────────────────────────────────────────────────

    #[test]
    fn whitespace_between_tokens_is_skipped() {
        let kinds = tokens("a   b");
        assert_eq!(kinds.len(), 2);
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "a"));
        assert!(matches!(&kinds[1], TokenKind::Identifier(s) if s == "b"));
    }

    #[test]
    fn tabs_are_treated_as_whitespace() {
        let kinds = tokens("a\tb");
        assert_eq!(kinds.len(), 2);
    }

    #[test]
    fn newlines_are_treated_as_whitespace() {
        let kinds = tokens("a\nb\rc\r\nd");
        assert_eq!(kinds.len(), 4);
    }

    #[test]
    fn vertical_tab_and_form_feed_are_whitespace() {
        let kinds = tokens("a\u{000B}b\u{000C}c");
        assert_eq!(kinds.len(), 3);
    }

    // ── identifiers ───────────────────────────────────────────────────────────

    #[test]
    fn identifier_ascii_is_lexed() {
        let kinds = tokens("hello");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "hello"));
    }

    #[test]
    fn identifier_with_underscore_is_lexed() {
        let kinds = tokens("_foo_bar_");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "_foo_bar_"));
    }

    #[test]
    fn identifier_with_digits_after_start_is_lexed() {
        let kinds = tokens("a1b2c3");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "a1b2c3"));
    }

    #[test]
    fn identifier_cannot_start_with_digit() {
        // '1' starts a number, not an identifier; 'a' is a separate identifier
        let kinds = tokens("1a");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "1"));
        assert!(matches!(&kinds[1], TokenKind::Identifier(s) if s == "a"));
    }

    #[test]
    fn identifier_greek_is_lexed() {
        let kinds = tokens("αβγ");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "αβγ"));
    }

    #[test]
    fn identifier_cjk_is_lexed() {
        let kinds = tokens("中文");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "中文"));
    }

    #[test]
    fn identifier_emoji_is_lexed() {
        // 🎯 is So (other symbol), NOT in XID_Start → rejected under UAX#31.
        // Use a Unicode mathematical letter instead, which IS XID_Start.
        // U+1D400 MATHEMATICAL BOLD CAPITAL A is in the Lo/Lm/Lu range and
        // has XID_Start=true.
        let kinds = tokens("\u{1D400}");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "\u{1D400}"));
    }

    #[test]
    fn identifier_is_split_at_reserved_symbol() {
        // '+' is reserved so "a+b" becomes three tokens
        let kinds = tokens("a+b");
        assert_eq!(kinds.len(), 3);
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "a"));
        assert!(matches!(&kinds[1], TokenKind::Symbol(Symbol::Plus)));
        assert!(matches!(&kinds[2], TokenKind::Identifier(s) if s == "b"));
    }

    #[test]
    fn identifier_is_split_at_dot() {
        let kinds = tokens("a.b");
        assert_eq!(kinds.len(), 3);
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "a"));
        assert!(matches!(&kinds[1], TokenKind::Symbol(Symbol::Dot)));
        assert!(matches!(&kinds[2], TokenKind::Identifier(s) if s == "b"));
    }

    // ── keywords ──────────────────────────────────────────────────────────────

    // Each keyword must be recognised as its own variant, not as an Identifier.
    macro_rules! keyword_test {
        ($name:ident, $text:literal, $variant:ident) => {
            #[test]
            fn $name() {
                let kinds = tokens($text);
                assert_eq!(kinds.len(), 1);
                assert!(matches!(kinds[0], TokenKind::Keyword(Keyword::$variant)));
            }
        };
    }

    keyword_test!(keyword_and_is_lexed, "and", And);
    keyword_test!(keyword_as_is_lexed, "as", As);
    keyword_test!(keyword_break_is_lexed, "break", Break);
    keyword_test!(keyword_const_is_lexed, "const", Const);
    keyword_test!(keyword_continue_is_lexed, "continue", Continue);
    keyword_test!(keyword_define_is_lexed, "define", Define);
    keyword_test!(keyword_do_is_lexed, "do", Do);
    keyword_test!(keyword_else_is_lexed, "else", Else);
    keyword_test!(keyword_elseif_is_lexed, "elseif", ElseIf);
    keyword_test!(keyword_end_is_lexed, "end", End);
    keyword_test!(keyword_enum_is_lexed, "enum", Enum);
    keyword_test!(keyword_export_is_lexed, "export", Export);
    keyword_test!(keyword_false_is_lexed, "false", False);
    keyword_test!(keyword_for_is_lexed, "for", For);
    keyword_test!(keyword_goto_is_lexed, "goto", Goto);
    keyword_test!(keyword_if_is_lexed, "if", If);
    keyword_test!(keyword_implement_is_lexed, "implement", Implement);
    keyword_test!(keyword_in_is_lexed, "in", In);
    keyword_test!(keyword_namespace_is_lexed, "namespace", Namespace);
    keyword_test!(keyword_nil_is_lexed, "nil", Nil);
    keyword_test!(keyword_not_is_lexed, "not", Not);
    keyword_test!(keyword_or_is_lexed, "or", Or);
    keyword_test!(keyword_repeat_is_lexed, "repeat", Repeat);
    keyword_test!(keyword_return_is_lexed, "return", Return);
    keyword_test!(keyword_struct_is_lexed, "struct", Struct);
    keyword_test!(keyword_then_is_lexed, "then", Then);
    keyword_test!(keyword_trait_is_lexed, "trait", Trait);
    keyword_test!(keyword_true_is_lexed, "true", True);
    keyword_test!(keyword_until_is_lexed, "until", Until);
    keyword_test!(keyword_use_is_lexed, "use", Use);
    keyword_test!(keyword_val_is_lexed, "val", Val);
    keyword_test!(keyword_var_is_lexed, "var", Var);
    keyword_test!(keyword_variant_is_lexed, "variant", Variant);
    keyword_test!(keyword_while_is_lexed, "while", While);

    #[test]
    fn keyword_prefix_is_not_a_keyword() {
        // "android" starts with "and" but is an identifier
        let kinds = tokens("android");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "android"));
    }

    #[test]
    fn keyword_with_trailing_chars_is_not_a_keyword() {
        let kinds = tokens("returns");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "returns"));
    }

    // ── symbols ───────────────────────────────────────────────────────────────

    macro_rules! symbol_test {
        ($name:ident, $text:literal, $variant:ident) => {
            #[test]
            fn $name() {
                let kinds = tokens($text);
                assert_eq!(kinds.len(), 1, "expected exactly one token for {:?}", $text);
                assert!(
                    matches!(kinds[0], TokenKind::Symbol(Symbol::$variant)),
                    "expected Symbol::{} for {:?}, got {:?}",
                    stringify!($variant),
                    $text,
                    kinds[0]
                );
            }
        };
    }

    symbol_test!(symbol_plus_is_lexed, "+", Plus);
    symbol_test!(symbol_minus_is_lexed, "-", Minus);
    symbol_test!(symbol_star_is_lexed, "*", Star);
    symbol_test!(symbol_slash_is_lexed, "/", Slash);
    symbol_test!(symbol_percent_is_lexed, "%", Percent);
    symbol_test!(symbol_caret_is_lexed, "^", Caret);
    symbol_test!(symbol_hash_is_lexed, "#", Hash);
    symbol_test!(symbol_amp_is_lexed, "&", Amp);
    symbol_test!(symbol_tilde_is_lexed, "~", Tilde);
    symbol_test!(symbol_pipe_is_lexed, "|", Pipe);
    symbol_test!(symbol_less_is_lexed, "<", Less);
    symbol_test!(symbol_greater_is_lexed, ">", Greater);
    symbol_test!(symbol_assign_is_lexed, "=", Assign);
    symbol_test!(symbol_lparen_is_lexed, "(", LParen);
    symbol_test!(symbol_rparen_is_lexed, ")", RParen);
    symbol_test!(symbol_lbrace_is_lexed, "{", LBrace);
    symbol_test!(symbol_rbrace_is_lexed, "}", RBrace);
    symbol_test!(symbol_lbracket_is_lexed, "[", LBracket);
    symbol_test!(symbol_rbracket_is_lexed, "]", RBracket);
    symbol_test!(symbol_semi_is_lexed, ";", Semi);
    symbol_test!(symbol_colon_is_lexed, ":", Colon);
    symbol_test!(symbol_comma_is_lexed, ",", Comma);
    symbol_test!(symbol_dot_is_lexed, ".", Dot);
    symbol_test!(symbol_at_is_lexed, "@", At);
    symbol_test!(symbol_dotdot_is_lexed, "..", DotDot);
    symbol_test!(symbol_eqeq_is_lexed, "==", EqEq);
    symbol_test!(symbol_noteq_is_lexed, "~=", NotEq);
    symbol_test!(symbol_lesseq_is_lexed, "<=", LessEq);
    symbol_test!(symbol_greatereq_is_lexed, ">=", GreaterEq);
    symbol_test!(symbol_shiftleft_is_lexed, "<<", ShiftLeft);
    symbol_test!(symbol_shiftright_is_lexed, ">>", ShiftRight);
    symbol_test!(symbol_floordiv_is_lexed, "//", FloorDiv);

    #[test]
    fn symbol_dotdot_is_preferred_over_two_dots() {
        // ".." must produce DotDot, not two Dot tokens
        let kinds = tokens("..");
        assert_eq!(kinds.len(), 1);
        assert!(matches!(kinds[0], TokenKind::Symbol(Symbol::DotDot)));
    }

    #[test]
    fn symbol_eqeq_is_preferred_over_two_assigns() {
        let kinds = tokens("==");
        assert_eq!(kinds.len(), 1);
        assert!(matches!(kinds[0], TokenKind::Symbol(Symbol::EqEq)));
    }

    #[test]
    fn symbol_floordiv_is_preferred_over_two_slashes() {
        let kinds = tokens("//");
        assert_eq!(kinds.len(), 1);
        assert!(matches!(kinds[0], TokenKind::Symbol(Symbol::FloorDiv)));
    }

    #[test]
    fn symbol_single_dot_before_letter_is_dot() {
        // ".x" — dot then identifier, not DotDot
        let kinds = tokens(".x");
        assert!(matches!(kinds[0], TokenKind::Symbol(Symbol::Dot)));
        assert!(matches!(&kinds[1], TokenKind::Identifier(s) if s == "x"));
    }

    // ── integer literals ──────────────────────────────────────────────────────

    #[test]
    fn integer_decimal_is_lexed() {
        let kinds = tokens("42");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "42"));
    }

    #[test]
    fn integer_zero_is_lexed() {
        let kinds = tokens("0");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "0"));
    }

    #[test]
    fn integer_hex_lowercase_is_lexed() {
        let kinds = tokens("0xff");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "0xff"));
    }

    #[test]
    fn integer_hex_uppercase_prefix_is_lexed() {
        let kinds = tokens("0XFF");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "0XFF"));
    }

    #[test]
    fn integer_hex_mixed_digits_is_lexed() {
        let kinds = tokens("0xDeAdBeEf");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "0xDeAdBeEf"));
    }

    // ── float literals ────────────────────────────────────────────────────────

    #[test]
    fn float_simple_is_lexed() {
        let kinds = tokens("3.14");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "3.14"));
    }

    #[test]
    fn float_leading_dot_is_lexed() {
        let kinds = tokens(".5");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == ".5"));
    }

    #[test]
    fn float_with_exponent_is_lexed() {
        let kinds = tokens("1e10");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "1e10"));
    }

    #[test]
    fn float_with_signed_exponent_is_lexed() {
        let kinds = tokens("1.5e-3");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "1.5e-3"));
    }

    #[test]
    fn float_with_uppercase_exponent_is_lexed() {
        let kinds = tokens("2E4");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "2E4"));
    }

    #[test]
    fn float_hex_with_exponent_is_lexed() {
        let kinds = tokens("0x1.8p+1");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "0x1.8p+1"));
    }

    #[test]
    fn float_hex_without_fraction_and_exponent_is_lexed() {
        let kinds = tokens("0x1p10");
        assert!(matches!(&kinds[0], TokenKind::Number(s) if s == "0x1p10"));
    }

    // ── short string literals ─────────────────────────────────────────────────

    #[test]
    fn string_double_quoted_is_lexed() {
        let kinds = tokens("\"hello\"");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "hello"));
    }

    #[test]
    fn string_single_quoted_is_lexed() {
        let kinds = tokens("'world'");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "world"));
    }

    #[test]
    fn string_empty_is_lexed() {
        let kinds = tokens("\"\"");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s.is_empty()));
    }

    #[test]
    fn string_with_escape_sequence_is_lexed() {
        let kinds = tokens(r#""a\nb""#);
        // The raw escape is preserved verbatim by the lexer
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == r"a\nb"));
    }

    #[test]
    fn string_with_backslash_quote_escape_is_lexed() {
        let kinds = tokens(r#""a\"b""#);
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == r#"a\"b"#));
    }

    #[test]
    fn string_unicode_content_is_lexed() {
        let kinds = tokens("\"αβγ\"");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "αβγ"));
    }

    #[test]
    fn string_newline_in_body_is_rejected() {
        must_fail("\"line1\nline2\"");
    }

    #[test]
    fn string_unterminated_is_rejected() {
        must_fail("\"hello");
    }

    #[test]
    fn string_unterminated_single_quote_is_rejected() {
        must_fail("'hello");
    }

    // ── long strings ──────────────────────────────────────────────────────────

    #[test]
    fn long_string_level_0_is_lexed() {
        let kinds = tokens("[[hello]]");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "hello"));
    }

    #[test]
    fn long_string_level_1_is_lexed() {
        let kinds = tokens("[=[hello]=]");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "hello"));
    }

    #[test]
    fn long_string_level_2_is_lexed() {
        let kinds = tokens("[==[hello]==]");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "hello"));
    }

    #[test]
    fn long_string_preserves_internal_newlines() {
        let kinds = tokens("[[a\nb\nc]]");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "a\nb\nc"));
    }

    #[test]
    fn long_string_skips_first_newline() {
        // A newline immediately after the opening bracket is discarded
        let kinds = tokens("[[\nhello]]");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "hello"));
    }

    #[test]
    fn long_string_does_not_close_on_mismatched_level() {
        // [[ ... ]=] does not close a level-0 string
        let kinds = tokens("[[a]=]b]]");
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "a]=]b"));
    }

    #[test]
    fn long_string_unterminated_is_rejected() {
        must_fail("[[hello");
    }

    #[test]
    fn long_string_can_contain_double_quotes() {
        let kinds = tokens(r#"[["hello"]]"#);
        assert!(matches!(&kinds[0], TokenKind::StringLiteral(s) if s == "\"hello\""));
    }

    // ── line comments ─────────────────────────────────────────────────────────

    #[test]
    fn line_comment_is_collected() {
        let r = lex("-- hello").unwrap();
        assert_eq!(r.comments.len(), 1);
        assert!(matches!(r.comments[0].kind, CommentKind::Line));
        assert!(r.comments[0].text.contains("hello"));
    }

    #[test]
    fn line_comment_does_not_produce_a_token() {
        let kinds = tokens("-- comment\nx");
        assert_eq!(kinds.len(), 1);
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "x"));
    }

    #[test]
    fn line_comment_ends_at_lf() {
        let r = lex("-- a\n-- b").unwrap();
        assert_eq!(r.comments.len(), 2);
    }

    #[test]
    fn line_comment_ends_at_cr() {
        let r = lex("-- a\r-- b").unwrap();
        assert_eq!(r.comments.len(), 2);
    }

    #[test]
    fn line_comment_ends_at_crlf() {
        let r = lex("-- a\r\n-- b").unwrap();
        assert_eq!(r.comments.len(), 2);
    }

    #[test]
    fn line_comment_text_includes_dashes() {
        let r = lex("-- note").unwrap();
        assert!(r.comments[0].text.starts_with("--"));
    }

    #[test]
    fn multiple_line_comments_preserve_order() {
        let r = lex("-- first\n-- second\n-- third").unwrap();
        assert_eq!(r.comments.len(), 3);
        assert!(r.comments[0].text.contains("first"));
        assert!(r.comments[1].text.contains("second"));
        assert!(r.comments[2].text.contains("third"));
    }

    // ── block comments ────────────────────────────────────────────────────────

    #[test]
    fn block_comment_level_0_is_collected() {
        let r = lex("--[[block]]").unwrap();
        assert_eq!(r.comments.len(), 1);
        assert!(matches!(r.comments[0].kind, CommentKind::Block));
    }

    #[test]
    fn block_comment_level_1_is_collected() {
        let r = lex("--[=[block]=]").unwrap();
        assert_eq!(r.comments.len(), 1);
        assert!(matches!(r.comments[0].kind, CommentKind::Block));
    }

    #[test]
    fn block_comment_does_not_produce_a_token() {
        let kinds = tokens("--[[ignored]] x");
        assert_eq!(kinds.len(), 1);
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "x"));
    }

    #[test]
    fn block_comment_can_span_multiple_lines() {
        let r = lex("--[[\nline1\nline2\n]]").unwrap();
        assert_eq!(r.comments.len(), 1);
        assert!(r.comments[0].text.contains("line1"));
    }

    #[test]
    fn block_comment_does_not_close_on_mismatched_level() {
        // --[[ ... ]=] should not close a level-0 block comment
        let r = lex("--[[a]=]b]]").unwrap();
        assert_eq!(r.comments.len(), 1);
        assert!(r.comments[0].text.contains("a]=]b"));
    }

    #[test]
    fn block_comment_text_includes_opening_dashes_and_brackets() {
        let r = lex("--[[text]]").unwrap();
        assert!(r.comments[0].text.starts_with("--[["));
    }

    #[test]
    fn block_comment_unterminated_is_rejected() {
        must_fail("--[[not closed");
    }

    #[test]
    fn mixed_line_and_block_comments_preserve_order() {
        let r = lex("-- line\n--[[block]]\n-- line2").unwrap();
        assert_eq!(r.comments.len(), 3);
        assert!(matches!(r.comments[0].kind, CommentKind::Line));
        assert!(matches!(r.comments[1].kind, CommentKind::Block));
        assert!(matches!(r.comments[2].kind, CommentKind::Line));
    }

    // ── comment vs token interaction ──────────────────────────────────────────

    #[test]
    fn comment_between_tokens_does_not_affect_token_stream() {
        let kinds = tokens("a -- comment\nb");
        assert_eq!(kinds.len(), 2);
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "a"));
        assert!(matches!(&kinds[1], TokenKind::Identifier(s) if s == "b"));
    }

    #[test]
    fn tokens_after_block_comment_are_lexed() {
        let kinds = tokens("--[[skip]] hello");
        assert!(matches!(&kinds[0], TokenKind::Identifier(s) if s == "hello"));
    }

    // ── rejection: control characters ────────────────────────────────────────
    //
    // Characters that are rejected reach scan_symbol (they are not whitespace,
    // not ident-start, not a digit, not a quote, not '[') and are not in the
    // 32-entry reserved-symbol table, so scan_symbol returns an error.
    //
    // Rejected classes:
    //   - ASCII C0 controls that are not whitespace (\x00-\x08, \x0E-\x1F, \x7F)
    //   - C1 Unicode controls (\u{0080}-\u{009F})
    //   - Unicode format/zero-width chars that Rust considers is_control()
    //     e.g. \u{00AD} soft-hyphen, \u{200B} zero-width space (is_whitespace)
    //
    // Note: visible non-ASCII chars that are NOT control and NOT reserved are
    // accepted as identifier characters.  The tests below also verify that
    // valid tokens surrounding a bad char are still parsed correctly up to the
    // bad char (i.e. the error position is meaningful).

    /// Assert that `input` fails and that the error byte-offset equals `byte`.
    fn must_fail_at(input: &str, byte: usize) {
        let e = lex(input).expect_err("expected lex to fail");
        assert_eq!(
            e.position.byte(),
            byte,
            "wrong error position for input {:?}: expected byte {}, got {}",
            input,
            byte,
            e.position.byte()
        );
    }

    // -- ASCII C0 control characters (non-whitespace) -------------------------

    #[test]
    fn reject_null_byte() {
        must_fail("\x00");
    }

    #[test]
    fn reject_soh_control() {
        must_fail("\x01");
    }

    #[test]
    fn reject_bel_control() {
        must_fail("\x07");
    }

    #[test]
    fn reject_bs_control() {
        must_fail("\x08");
    }

    #[test]
    fn reject_so_control() {
        must_fail("\x0E");
    }

    #[test]
    fn reject_us_control() {
        must_fail("\x1F");
    }

    #[test]
    fn reject_del_control() {
        must_fail("\x7F");
    }

    // -- ASCII C0 controls mixed with valid tokens ----------------------------

    #[test]
    fn reject_null_byte_after_valid_identifier() {
        // "abc" lexes fine; \x00 at byte 3 is where the error should fire
        must_fail_at("abc\x00def", 3);
    }

    #[test]
    fn reject_control_between_two_identifiers() {
        // valid "x", then \x01, then valid "y" — error at byte 1
        must_fail_at("x\x01y", 1);
    }

    #[test]
    fn reject_del_at_start() {
        must_fail_at("\x7Fabc", 0);
    }

    #[test]
    fn reject_control_after_number() {
        // "42" is a valid number token; \x0E follows at byte 2
        must_fail_at("42\x0E", 2);
    }

    #[test]
    fn reject_control_after_string_literal() {
        // `"hi"` is 4 bytes; \x08 at byte 4
        must_fail_at("\"hi\"\x08", 4);
    }

    #[test]
    fn reject_control_after_symbol() {
        // '+' is 1 byte; \x00 at byte 1
        must_fail_at("+\x00", 1);
    }

    // -- spans: valid tokens before the bad char are correctly produced --------

    #[test]
    fn valid_tokens_before_control_are_correct() {
        // We can't use the `tokens` helper (it panics on failure), so we call
        // lex directly and inspect the partial result.  The lexer is not
        // streaming — it fails on the first bad character — so we verify the
        // error is at the right place and that lexing the prefix succeeds.
        let prefix = "hello ";
        let full = format!("{}\x00tail", prefix);
        let err = lex(&full).unwrap_err();
        // error position is right after the 6-byte prefix
        assert_eq!(err.position.byte(), prefix.len());
        // the prefix alone lexes cleanly
        let r = lex(prefix.trim()).unwrap();
        assert!(matches!(&r.tokens[0].kind, TokenKind::Identifier(s) if s == "hello"));
    }

    // -- C1 Unicode control block (\u{0080}–\u{009F}) -------------------------

    #[test]
    fn reject_c1_control_pad() {
        // U+0080 PAD — first C1 control
        must_fail("\u{0080}");
    }

    #[test]
    fn reject_c1_control_nel() {
        // U+0085 NEL — is_control()=true in Rust.
        // advance_trivia does not recognise it as a newline, so it reaches
        // scan_token → scan_symbol → not in symbol table → error.
        must_fail("\u{0085}");
    }

    #[test]
    fn reject_c1_control_nel_after_valid_token() {
        // "ok" then U+0085 at byte 2
        must_fail_at("ok\u{0085}", 2);
    }

    #[test]
    fn reject_c1_control_sos() {
        // U+0098 SOS — is_control()=true, not whitespace → scan_symbol error
        must_fail("\u{0098}");
    }

    #[test]
    fn reject_c1_control_apc() {
        // U+009F APC — is_control()=true → error
        must_fail("\u{009F}");
    }

    #[test]
    fn reject_c1_control_mixed_with_valid_ascii() {
        // "ok" then U+0081 at byte 2
        must_fail_at("ok\u{0081}", 2);
    }

    // -- Unicode format / bidi / invisible characters -------------------------
    //
    // Under UAX#31 / C++26:
    //   - XID_Start / XID_Continue are derived from Unicode General Category,
    //     excluding Pattern_Syntax and Pattern_White_Space.
    //   - Format characters (Cf) such as U+200C..200F, U+FFF9, U+FEFF are NOT
    //     in XID_Start or XID_Continue and are therefore rejected.
    //   - U+00AD SOFT HYPHEN is a Cf character, also rejected.
    //   - U+2028 LINE SEPARATOR and U+2029 PARAGRAPH SEPARATOR have
    //     Pattern_White_Space=true, so they fail is_ident_start and reach
    //     scan_symbol, which also rejects them.

    #[test]
    fn reject_zero_width_non_joiner_u200c() {
        // U+200C Cf — not XID_Start → rejected
        must_fail("\u{200C}");
    }

    #[test]
    fn reject_zero_width_joiner_u200d() {
        // U+200D Cf — not XID_Start → rejected
        must_fail("\u{200D}");
    }

    #[test]
    fn reject_left_to_right_mark_u200e() {
        // U+200E Cf — not XID_Start → rejected
        must_fail("\u{200E}");
    }

    #[test]
    fn reject_right_to_left_mark_u200f() {
        // U+200F Cf — not XID_Start → rejected
        must_fail("\u{200F}");
    }

    #[test]
    fn reject_interlinear_annotation_anchor_ufff9() {
        // U+FFF9 Cf — not XID_Start → rejected
        must_fail("\u{FFF9}");
    }

    #[test]
    fn reject_soft_hyphen_u00ad() {
        // U+00AD Cf — not XID_Start → rejected
        must_fail("\u{00AD}");
    }

    #[test]
    fn reject_bom_ufeff() {
        // U+FEFF Cf — not XID_Start → rejected
        must_fail("\u{FEFF}");
    }

    #[test]
    fn reject_line_separator_u2028() {
        // U+2028 Pattern_White_Space=true → fails is_ident_start → scan_symbol
        // → not in symbol table → error.
        must_fail("\u{2028}");
    }

    #[test]
    fn reject_paragraph_separator_u2029() {
        must_fail("\u{2029}");
    }

    #[test]
    fn reject_line_separator_after_valid_token() {
        must_fail_at("ab\u{2028}", 2);
    }

    #[test]
    fn reject_paragraph_separator_after_valid_token() {
        must_fail_at("ab\u{2029}", 2);
    }

    #[test]
    fn reject_format_char_after_valid_identifier() {
        // U+200E LEFT-TO-RIGHT MARK is Cf and NOT in XID_Continue →
        // it terminates "ab" and then fails scan_symbol → error at byte 2.
        must_fail_at("ab\u{200E}", 2);
    }

    #[test]
    fn zwnj_is_valid_identifier_continue_char() {
        // U+200C ZWNJ is in Other_ID_Continue per UAX#31 →
        // it is a valid XID_Continue character, so "ab\u{200C}" is one identifier.
        let r = lex("ab\u{200C}").unwrap();
        assert!(matches!(&r.tokens[0].kind,
            TokenKind::Identifier(s) if s == "ab\u{200C}"));
    }

    #[test]
    fn zwj_is_valid_identifier_continue_char() {
        // U+200D ZWJ is in Other_ID_Continue per UAX#31 → valid XID_Continue.
        let r = lex("ab\u{200D}").unwrap();
        assert!(matches!(&r.tokens[0].kind,
            TokenKind::Identifier(s) if s == "ab\u{200D}"));
    }

    // -- confirm visible non-ASCII letters ARE accepted -----------------------
    //
    // Under UAX#31, characters with XID_Start are accepted.  This covers all
    // Unicode letters (Lu, Ll, Lt, Lm, Lo) and letterlike numbers (Nl).

    #[test]
    fn visible_non_ascii_letter_is_accepted() {
        let r = lex("é").unwrap();
        assert!(matches!(&r.tokens[0].kind, TokenKind::Identifier(s) if s == "é"));
    }

    #[test]
    fn visible_non_ascii_cjk_is_accepted() {
        let r = lex("字").unwrap();
        assert!(matches!(&r.tokens[0].kind, TokenKind::Identifier(s) if s == "字"));
    }

    // -- confirm non-letter visible chars are correctly rejected --------------
    //
    // Emojis are Emoji_Presentation (So), not letters → NOT in XID_Start.
    // ASCII punctuation not in the reserved-symbol table but covered by
    // Pattern_Syntax (!, ?, $, `) are also excluded by UAX#31.

    #[test]
    fn reject_emoji_as_identifier_start() {
        // 🚀 is So (other symbol), not XID_Start → rejected
        must_fail("🚀");
    }

    #[test]
    fn reject_ascii_backtick_as_identifier_start() {
        // U+0060 GRAVE ACCENT is Pattern_Syntax → rejected
        must_fail("`foo`");
    }

    #[test]
    fn reject_ascii_exclamation_as_identifier_start() {
        // U+0021 is Pattern_Syntax → rejected
        must_fail("ok!");
    }

    #[test]
    fn reject_ascii_question_mark_as_identifier_start() {
        // U+003F is Pattern_Syntax → rejected
        must_fail("what?");
    }

    #[test]
    fn reject_ascii_dollar_as_identifier_start() {
        // U+0024 is Pattern_Syntax → rejected
        must_fail("$price");
    }
}
