mod lexer;
mod token;

#[allow(unused)]
pub use lexer::lex;

#[allow(unused)]
pub use token::{Keyword, Position, Span, Symbol, Token, TokenKind, Trivia, TriviaKind};

