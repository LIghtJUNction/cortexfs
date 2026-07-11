#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    cortexfs::mount::main()
}
