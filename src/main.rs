mod lexical;
mod syntax;

use crate::lexical::lexer::Lexer;
use crate::syntax::emit::Emitter;
use crate::syntax::parser::Parser;
use std::io::{self, Read};

fn main() {
    let mut input = String::new();
    if let Err(err) = io::stdin().read_to_string(&mut input) {
        eprintln!("Failed to read stdin: {err}");
        std::process::exit(1);
    }

    let tokens = match Lexer::new(&input).lex_all() {
        Ok(tokens) => tokens,
        Err(err) => {
            eprintln!("Lex error at line {} column {}: {}", err.position.line, err.position.column, err.message);
            std::process::exit(1);
        }
    };

    let chunk = match Parser::new(tokens).parse_chunk() {
        Ok(chunk) => chunk,
        Err(err) => {
            eprintln!("Parse error at line {} column {}: {}", err.position.line, err.position.column, err.message);
            std::process::exit(1);
        }
    };

    let output = Emitter::emit_chunk(&chunk);
    print!("{output}");
}

#[test]
fn test_full() {
    let code = r#"
    -- Short, feature-rich Nova snippet for compiler testing

use System;
namespace Example;

export define Pair struct
  a: integer;
  b: integer;
end

export define add (x: integer, y: integer): integer
  return x + y
end

export define demo (): integer
  var p = Pair { a = 1, b = 2 }
  var sum = add(p.a, p.b)
  if sum > 2 then
    return sum
  else
    return 0
  end
end
    "#;
    let tokens = match Lexer::new(code).lex_all() {
        Ok(tokens) => tokens,
        Err(err) => {
            eprintln!("Lex error at line {} column {}: {}", err.position.line, err.position.column, err.message);
            std::process::exit(1);
        }
    };

    let chunk = match Parser::new(tokens).parse_chunk() {
        Ok(chunk) => chunk,
        Err(err) => {
            eprintln!("Parse error at line {} column {}: {}", err.position.line, err.position.column, err.message);
            std::process::exit(1);
        }
    };

    let output = Emitter::emit_chunk(&chunk);
    print!("{output}");
}