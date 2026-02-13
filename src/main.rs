mod ast;
mod emit;
mod lexer;

mod parser;
mod token;

use crate::emit::Emitter;
use crate::lexer::Lexer;
use crate::parser::Parser;
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
    -- Short, feature-rich Lua snippet for compiler testing

local function fold(tbl, init, f, ...)
  local acc = init
  for i = 1, #tbl do
    acc = f(acc, tbl[i], ...)
  end
  return acc
end

local function make_adder(x)
  return function(y) return x + y end
end

local t = { 1, 2, 3, 4, 5 }
local add = make_adder(10)

local sum = fold(t, 0, function(a, b) return a + b end)
local mapped = {}
for i, v in ipairs(t) do
  mapped[i] = add(v)
end

local function classify(n)
  if n % 2 == 0 then
    return "even"
  elseif n % 3 == 0 then
    return "div3"
  else
    return "other"
  end
end

local stats = { even = 0, div3 = 0, other = 0 }
for _, v in ipairs(mapped) do
  local k = classify(v)
  stats[k] = stats[k] + 1
end

local msg = string.format("sum=%d, even=%d, div3=%d, other=%d",
  sum, stats.even, stats.div3, stats.other)

print(msg)
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