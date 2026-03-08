//! Expected error: "argot field must include either `positional` or `flag`"
use argot::ArgotCommand;

#[derive(ArgotCommand)]
struct Bad {
    #[argot(description = "no kind specified")]
    field: String,
}

fn main() {}
