using Microsoft.Diagnostics.Tracing.Etlx;

if (args.Length is < 1 or > 3)
{
    Console.Error.WriteLine(
        "usage: rustc-memory <trace.etl|trace.etlx> [process-name] [rustc-driver.dll]");
    return 2;
}

var tracePath = Path.GetFullPath(args[0]);
var processName = args.Length >= 2 ? args[1] : "rustc";
var explicitModulePath = args.Length == 3 ? Path.GetFullPath(args[2]) : null;

using var trace = TraceLog.OpenOrConvert(tracePath);
var processes = trace.Processes
    .Where(process => string.Equals(process.Name, processName, StringComparison.OrdinalIgnoreCase))
    .ToArray();

if (processes.Length == 0)
{
    Console.Error.WriteLine($"no process named '{processName}' exists in {tracePath}");
    return 3;
}

foreach (var process in processes)
{
    var analysis = VirtualCommitAnalyzer.Analyze(process);
    var modulePath = explicitModulePath ?? analysis.FindRustcDriverPath();
    var symbols = modulePath is null
        ? GnuSymbolResolver.Empty
        : GnuSymbolResolver.Resolve(modulePath, analysis.RustcDriverRvasAtPeak());

    analysis.WriteReport(Console.Out, symbols, stackLimit: 20, frameLimit: 12);
}

return 0;
