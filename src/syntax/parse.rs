use crate::lexical::{Keyword, Symbol, Token, TokenKind, Trivia as LexTrivia};
use crate::syntax::ast::{Chunk, Trivia};
use crate::syntax::syntax::SyntaxError;

pub(super) struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) index: usize,
    pub(crate) trivia: Trivia,
}

pub(super) trait Parsable: Sized {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError>;
}

impl Parser {
    pub fn new(tokens: Vec<Token>, trivia: Vec<LexTrivia>) -> Self {
        Self {
            tokens,
            index: 0,
            trivia,
        }
    }

    pub fn parse_chunk(mut self) -> Result<Chunk, SyntaxError> {
        Chunk::parse(&mut self)
    }

    pub(crate) fn advance(&mut self) -> Token {
        let token = self.current().clone();
        self.index += 1;
        token
    }

    pub(crate) fn checkpoint(&self) -> usize {
        self.index
    }

    pub(crate) fn restore(&mut self, checkpoint: usize) {
        self.index = checkpoint;
    }

    pub(crate) fn peek(&self, offset: usize) -> &Token {
        let idx = (self.index + offset).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    pub(crate) fn peek_is_symbol(&self, offset: usize, symbol: Symbol) -> bool {
        matches!(self.peek(offset).kind, TokenKind::Symbol(s) if s == symbol)
    }

    pub(crate) fn expect_symbol(&mut self, symbol: Symbol) -> Result<Token, SyntaxError> {
        let token = self.current().clone();
        if matches!(token.kind, TokenKind::Symbol(s) if s == symbol) {
            self.index += 1;
            return Ok(token);
        }
        Err(SyntaxError {
            message: format!("expected symbol {:?}", symbol),
            position: token.span.start,
        })
    }

    pub(crate) fn current(&self) -> &Token {
        &self.tokens[self.index]
    }

    pub(crate) fn is_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.current().kind, TokenKind::Keyword(k) if k == keyword)
    }

    pub(crate) fn is_symbol(&self, symbol: Symbol) -> bool {
        matches!(self.current().kind, TokenKind::Symbol(s) if s == symbol)
    }

    pub(crate) fn is_block_end(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Eof
                | TokenKind::Keyword(Keyword::End)
                | TokenKind::Keyword(Keyword::Else)
                | TokenKind::Keyword(Keyword::ElseIf)
                | TokenKind::Keyword(Keyword::Until)
        )
    }

    pub(crate) fn expect_keyword(&mut self, keyword: Keyword) -> Result<Token, SyntaxError> {
        let token = self.current().clone();
        if matches!(token.kind, TokenKind::Keyword(k) if k == keyword) {
            self.index += 1;
            return Ok(token);
        }
        Err(SyntaxError {
            message: format!("expected keyword {:?}", keyword),
            position: token.span.start,
        })
    }
}
