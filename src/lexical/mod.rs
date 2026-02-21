mod lexer;
mod token;

#[allow(dead_code)]
pub use lexer::{lex, LexError, LexResult};

#[allow(dead_code)]
pub use token::{Comment, CommentKind, Keyword, Position, Span, Symbol, Token, TokenKind};