//! Expected error: "argot field must include either `positional` or `flag`"
//! (currently derives positional when both are set — a bug to fix)
use argot::ArgotCommand;

#[derive(ArgotCommand)]
struct Bad {
    #[argot(positional, flag, description = "ambiguous")]
    field: String,
}

fn main() {}
