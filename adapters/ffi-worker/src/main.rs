use std::process::ExitCode;

#[global_allocator]
static GLOBAL_ALLOCATOR: rad_vm::allocation_meter::MeteredSystemAllocator =
    rad_vm::allocation_meter::MeteredSystemAllocator;

fn required_argument(args: &[String], name: &str) -> Result<String, String> {
    let index = args
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("missing required {name} argument"))?;
    args.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("missing value after {name}"))
}

fn run() -> Result<(), String> {
    rad_vm::allocation_meter::mark_allocator_installed();
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 5 {
        return Err("rad-ffi-worker requires --connect <address> --plugin <path>".to_string());
    }
    let connect = required_argument(&args, "--connect")?;
    let plugin = required_argument(&args, "--plugin")?;
    let token = std::env::var("RAD_FFI_WORKER_TOKEN")
        .map_err(|_| "RAD_FFI_WORKER_TOKEN is required".to_string())?;
    rad_vm::ffi::run_plugin_worker(&connect, &plugin, &token)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
