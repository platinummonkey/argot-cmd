//! Expected error: "ArgotCommand can only be derived for structs"
use argot::ArgotCommand;

#[derive(ArgotCommand)]
enum MyEnum { A, B }

fn main() {}
