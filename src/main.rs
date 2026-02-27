mod lexical;
mod syntax;
mod formats;

use std::io::{self, Read};

fn main() {
    let mut input = String::new();
    if let Err(err) = io::stdin().read_to_string(&mut input) {
        eprintln!("Failed to read stdin: {err}");
        std::process::exit(1);
    }

    let lex_result = match lexical::transform(&input) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Lexical error at line {} column {}: {}", err.position.line(), err.position.column(), err.message);
            std::process::exit(1);
        }
    };

    let syntax_result = match syntax::transform(lex_result) {
        Ok(chunk) => chunk,
        Err(err) => {
            eprintln!("Syntax error at line {} column {}: {}", err.position.line(), err.position.column(), err.message);
            std::process::exit(1);
        }
    };
}