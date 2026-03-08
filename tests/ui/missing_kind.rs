//! Expected error: "field `field` has `#[argot(...)]` but is missing a kind — add `positional` or `flag`"
use argot::ArgotCommand;

#[derive(ArgotCommand)]
struct Bad {
    #[argot(description = "no kind specified")]
    field: String,
}

fn main() {}
