fn main() {
    let drive = std::env::args().nth(1).unwrap_or_else(|| "C:".into());
    space_scope_lib::bench(&drive);
}
