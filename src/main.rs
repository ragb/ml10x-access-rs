use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("ml10x CLI not yet implemented (Phase F/G pending)");
    ExitCode::from(ml10x::exit_codes::GENERIC_ERROR as u8)
}
