use std::process::ExitCode;

#[global_allocator]
static GLOBAL_ALLOCATOR: rad_vm::allocation_meter::MeteredSystemAllocator =
    rad_vm::allocation_meter::MeteredSystemAllocator;

#[cfg(windows)]
fn disable_interactive_fault_handling() {
    // An untrusted DLL fault must terminate this worker immediately. Without
    // these process modes Windows Error Reporting can retain the crashing
    // process long enough for the parent socket deadline to misclassify the
    // fault as a live plugin hang (and may attempt UI on an unattended host).
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
        fn SetUnhandledExceptionFilter(
            filter: Option<unsafe extern "system" fn(*mut std::ffi::c_void) -> i32>,
        ) -> Option<unsafe extern "system" fn(*mut std::ffi::c_void) -> i32>;
    }

    // A worker is a disposable fault boundary. Returning
    // EXCEPTION_EXECUTE_HANDLER from its top-level filter terminates the
    // faulting process through Windows' exception path without entering the
    // interactive/error-reporting path. The parent owns the typed failure and
    // starts the next generation in a fresh process.
    unsafe extern "system" fn terminate_worker_on_fault(_: *mut std::ffi::c_void) -> i32 {
        1
    }
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
        SetUnhandledExceptionFilter(Some(terminate_worker_on_fault));
    }
}

#[cfg(not(windows))]
fn disable_interactive_fault_handling() {}

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
    disable_interactive_fault_handling();
    rad_vm::allocation_meter::mark_allocator_installed();
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() == 3 && args[1] == "--verify" {
        let report = rad_vm::ffi::verify_plugin(&args[2])?;
        println!(
            "{}",
            serde_json::to_string(&report)
                .map_err(|error| format!("cannot encode native verification report: {error}"))?
        );
        return Ok(());
    }
    if args.len() != 5 {
        return Err(
            "rad-ffi-worker requires --connect <address> --plugin <path> or --verify <path>"
                .to_string(),
        );
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
