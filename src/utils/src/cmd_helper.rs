use std::process::Command;

pub fn get_command_str(cmd: &Command) -> String {
    let prog = cmd.get_program().to_str().unwrap();
    let args: Vec<&str> = cmd.get_args().map(|x| x.to_str().unwrap()).collect();
    std::iter::once(prog)
        .chain(args)
        .collect::<Vec<_>>()
        .join(" ")
}
