#![allow(unused)]

mod lexer;
mod token;

pub use lexer::{LexicalError, LexicalResult, transform};

pub use token::{Keyword, Position, Span, Symbol, Token, TokenKind, Trivia, TriviaKind};
