//! Expected error: "a field cannot be both `positional` and `flag` — choose one"
use argot::ArgotCommand;

#[derive(ArgotCommand)]
struct Bad {
    #[argot(positional, flag, description = "ambiguous")]
    field: String,
}

fn main() {}
