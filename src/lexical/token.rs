use serde::{Serialize, Serializer};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Position {
    pub byte: usize,
    pub grapheme: usize,
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn start() -> Self {
        Self {
            byte: 0,
            grapheme: 0,
            line: 1,
            column: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl Serialize for Span {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}..{}", self.start.grapheme, self.end.grapheme))
    }
}

impl Span {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn single(pos: Position) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    pub fn merge(self, other: Span) -> Self {
        let start = if self.start.grapheme <= other.start.grapheme {
            self.start
        } else {
            other.start
        };
        let end = if self.end.grapheme >= other.end.grapheme {
            self.end
        } else {
            other.end
        };
        Self { start, end }
    }

    #[allow(dead_code)]
    pub fn len_graphemes(&self) -> usize {
        self.end.grapheme.saturating_sub(self.start.grapheme)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start.grapheme, self.end.grapheme)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum CommentKind {
    Line,
    Block,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Comment {
    pub kind: CommentKind,
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Keyword {
    And,
    As,
    Break,
    Const,
    Continue,
    Define,
    Do,
    Else,
    ElseIf,
    End,
    Enum,
    Export,
    False,
    For,
    Goto,
    If,
    Implement,
    In,
    Namespace,
    Nil,
    Not,
    Or,
    Repeat,
    Return,
    Struct,
    Then,
    Trait,
    True,
    Until,
    Use,
    Val,
    Var,
    Variant,
    While,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Symbol {
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Hash,
    Amp,
    Tilde,
    Pipe,
    ShiftLeft,
    ShiftRight,
    FloorDiv,
    EqEq,
    NotEq,
    LessEq,
    GreaterEq,
    Less,
    Greater,
    Assign,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semi,
    Colon,
    Comma,
    Dot,
    DotDot,
    At,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Eof,
    Identifier(String),
    Number(String),
    StringLiteral(String),
    Keyword(Keyword),
    Symbol(Symbol),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_merge_keeps_outer_bounds() {
        let a = Span::new(
            Position {
                byte: 0,
                grapheme: 0,
                line: 1,
                column: 0,
            },
            Position {
                byte: 2,
                grapheme: 2,
                line: 1,
                column: 2,
            },
        );
        let b = Span::new(
            Position {
                byte: 2,
                grapheme: 2,
                line: 1,
                column: 2,
            },
            Position {
                byte: 5,
                grapheme: 5,
                line: 1,
                column: 5,
            },
        );
        let merged = a.merge(b);
        assert_eq!(merged.start.grapheme, 0);
        assert_eq!(merged.end.grapheme, 5);
    }
}
