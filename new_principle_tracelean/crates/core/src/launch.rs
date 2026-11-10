use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

/// @implements REQ-HANDOFF.started_not_driven
pub fn start(argv: &[String]) -> Result<(), String> {
    let (program, args) = argv.split_first().ok_or("nothing to start")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|why| format!("cannot start {program}: {why}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// @implements REQ-HANDOFF.started_not_driven
pub fn ask(argv: &[String]) -> Receiver<Result<String, String>> {
    let (tell, answer) = channel();
    let argv = argv.to_vec();
    std::thread::spawn(move || {
        let said = match argv.split_first() {
            None => Err("nothing to start".to_string()),
            Some((program, args)) => Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map(|out| if out.status.success() { String::from_utf8_lossy(&out.stdout).into_owned() } else { String::new() })
                .map_err(|why| format!("cannot start {program}: {why}")),
        };
        let _ = tell.send(said);
    });
    answer
}
