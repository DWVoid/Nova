# Nova Grammar 

Notes:
- Terminals are quoted (e.g., "if").
- Non-terminals use angle brackets (e.g., <stat>).
- `{ ... }` means repetition (zero or more).
- `[ ... ]` means optional.

## 1. Compilation Unit

BNF:
```
<compilation_unit> ::= { <use_decl> } <namespace_decl> { <top_item> } <eof>

<top_item> ::= <definition> | <implementation>
```

## 2. Use and Namespace Declarations

BNF:
```
<use_decl> ::= "use" <namespace_path> [ <use_selector> | <use_alias> ] ";"

<use_selector> ::= "." "{" <use_list> "}"
<use_list> ::= <use_item> { "," <use_item> }
<use_item> ::= <name> [ "as" <name> ]

<use_alias> ::= "as" <name>

<namespace_decl> ::= "namespace" <namespace_path> ";"

<namespace_path> ::= <name> { "." <name> }
```

## 3. Visibility Modifiers

BNF:
```
<visibility> ::= "export" [ "(" <scope_list> ")" ]
<scope_list> ::= <name> { "," <name> }
```

## 4. Definitions and Implementations

BNF:
```
<definition> ::= { <decorator> } [ <visibility> ] "define" <name> [ <type_spec> ] <def_expr> [ ";" ]

<def_expr> ::= <struct_def>
             | <enum_def>
             | <variant_def>
             | <trait_def>
             | <exp>

<implementation> ::= "implement" [ <type> ] "for" <type> <impl_body> "end"
<impl_body> ::= { <definition> }
```

### 4.1 Structs

BNF:
```
<struct_def> ::= "struct" <struct_body> "end"
<struct_body> ::= { <field_decl> }
<field_decl> ::= <name> <type_spec> [ ";" ]
```

### 4.2 Enums

BNF:
```
<enum_def> ::= "enum" <type_spec> <enum_body> "end"
<enum_body> ::= { <enum_member> }
<enum_member> ::= <name> "=" <exp> [ ";" ]
```

### 4.3 Variants

BNF:
```
<variant_def> ::= "variant" <variant_body> "end"
<variant_body> ::= { <variant_member> }
<variant_member> ::= <name> <type_spec> [ ";" ]
```

### 4.4 Traits

BNF:
```
<trait_def> ::= "trait" <trait_body> "end"
<trait_body> ::= { <trait_sig> }
<trait_sig> ::= <name> <param_list> <type_spec> [ ";" ]
```

## 5. Decorators

BNF:
```
<decorator> ::= "@" <name> [ "(" [ <explist> ] ")" ]
```

## 6. Types

BNF:
```
<type> ::= <namespace_path>
<type_spec> ::= ":" <type>
```

## 7. Blocks and Statements

BNF:
```
<block> ::= { <stat> } [ <retstat> [ ";" ] ]

<stat> ::= ";"
        | <exp> { "," <exp> } "=" <explist>
        | <exp> <args>
        | <exp> ":" <name> <args>
        | "do" <block> "end"
        | "while" <exp> "do" <block> "end"
        | "repeat" <block> "until" <exp>
        | "if" <exp> "then" <block> { "elseif" <exp> "then" <block> } [ "else" <block> ] "end"
        | "for" <name> "=" <exp> "," <exp> [ "," <exp> ] "do" <block> "end"
        | "for" <namelist> "in" <explist> "do" <block> "end"
        | "break"
        | "continue"
        | "goto" <name>
        | "::" <name> "::"

<retstat> ::= "return" [ <explist> ]

<namelist> ::= <name> { "," <name> }
<explist>  ::= <exp> { "," <exp> }
```

The left-hand side of an assignment and the target of a call statement are
written as plain expressions.  The semantic stage enforces that assignment
targets are valid l-values (`<name>`, field access, index access, or a
`var`/`val` declaration) and that call statements resolve to an actual call.

## 8. Expressions

### 8.1 Expression Forms

All expression forms, including names, field/index access, calls, and local
variable declarations, are unified into a single `<exp>` rule.  The parser
makes no distinction between l-values and r-values — that analysis is deferred
to the semantic stage.

BNF:
```
<exp> ::= "nil"
        | "false"
        | "true"
        | <number>
        | <string>
        | <name>
        | <lambda_expr>
        | "var" <name> [ <type_spec> ]
        | "val" <name> [ <type_spec> ]
        | "(" <exp> ")"
        | <exp> "." <name>
        | <exp> "[" <exp> "]"
        | <exp> ":" <name> <args>
        | <exp> <args>
        | <unop> <exp>
        | <exp> <binop> <exp>
```

Postfix operators (`.`, `[]`, `:method`, and call arguments) bind tighter than
any prefix or binary operator and are left-associative.

The `var`/`val` forms introduce a local binding site.  They are syntactically
valid in any expression position but are semantically restricted to the
left-hand side of an assignment statement.

### 8.2 Lambda Expressions

BNF:
```
<lambda_expr> ::= [ "const" ] <param_list> <type_spec> <block> "end"

<param_list> ::= "(" [ <param_items> ] ")"
<param_items> ::= <param> { "," <param> }
<param> ::= <name> [ <type_spec> ]
```

### 8.3 Unary Operators

BNF:
```
<unop> ::= "not" | "-" | "#" | "~"
```

### 8.4 Binary Operators and Precedence

Precedence (lowest to highest), with associativity:
1. `or` (left)
2. `and` (left)
3. Comparisons: `<` `<=` `>` `>=` `==` `~=` (left)
4. Bitwise OR: `|` (left)
5. Bitwise XOR: `~` (left)
6. Bitwise AND: `&` (left)
7. Shifts: `<<` `>>` (left)
8. Concatenation: `..` (right)
9. Additive: `+` `-` (left)
10. Multiplicative: `*` `/` `//` `%` (left)
11. Exponentiation: `^` (right)

BNF (operator set):
```
<binop> ::= "or" | "and"
         | "<" | "<=" | ">" | ">=" | "==" | "~="
         | "|" | "~" | "&"
         | "<<" | ">>"
         | ".."
         | "+" | "-"
         | "*" | "/" | "//" | "%"
         | "^"
```

## 9. Initializers

BNF:
```
<initializer> ::= "{" [ <fieldlist> ] "}"
<fieldlist> ::= <field> { "," <field> } [ "," ]

<field> ::= "[" <exp> "]" "=" <exp>
          | <name> "=" <exp>
          | <exp>
```

## 10. Call Arguments

BNF:
```
<args> ::= "(" [ <explist> ] ")"
        | <initializer>
```