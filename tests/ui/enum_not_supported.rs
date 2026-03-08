//! Expected error: "`#[derive(ArgotCommand)]` cannot be used on enum `MyEnum` — only structs are supported"
use argot::ArgotCommand;

#[derive(ArgotCommand)]
enum MyEnum { A, B }

fn main() {}
