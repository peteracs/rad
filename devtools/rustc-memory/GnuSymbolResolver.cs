using System.Diagnostics;
using System.Globalization;
using System.Reflection.PortableExecutable;

internal sealed class GnuSymbolResolver
{
    private readonly IReadOnlyDictionary<ulong, ResolvedSymbol> symbols;

    private GnuSymbolResolver(IReadOnlyDictionary<ulong, ResolvedSymbol> symbols)
    {
        this.symbols = symbols;
    }

    public static GnuSymbolResolver Empty { get; } =
        new(new Dictionary<ulong, ResolvedSymbol>());

    public static GnuSymbolResolver Resolve(
        string modulePath,
        IReadOnlyCollection<ulong> relativeAddresses)
    {
        if (relativeAddresses.Count == 0)
        {
            return Empty;
        }

        using var module = File.OpenRead(modulePath);
        using var peReader = new PEReader(module);
        var imageBase = peReader.PEHeaders.PEHeader?.ImageBase
            ?? throw new InvalidDataException($"{modulePath} has no PE header");
        var targets = relativeAddresses
            .Distinct()
            .Select(relativeAddress => new SymbolTarget(relativeAddress, imageBase + relativeAddress))
            .OrderBy(target => target.AbsoluteAddress)
            .ToArray();

        var startInfo = new ProcessStartInfo
        {
            FileName = "nm.exe",
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };
        startInfo.ArgumentList.Add("-n");
        startInfo.ArgumentList.Add("-C");
        startInfo.ArgumentList.Add(modulePath);

        using var process = Process.Start(startInfo)
            ?? throw new InvalidOperationException("failed to start nm.exe");
        var errorTask = process.StandardError.ReadToEndAsync();
        var resolved = new Dictionary<ulong, ResolvedSymbol>(targets.Length);
        var targetIndex = 0;
        ulong nearestAddress = 0;
        var nearestName = "<no-lower-symbol>";

        while (process.StandardOutput.ReadLine() is { } line)
        {
            if (!TryParseSymbol(line, out var address, out var name))
            {
                continue;
            }

            while (targetIndex < targets.Length && targets[targetIndex].AbsoluteAddress < address)
            {
                var target = targets[targetIndex++];
                resolved[target.RelativeAddress] = new ResolvedSymbol(
                    nearestName,
                    target.AbsoluteAddress - nearestAddress);
            }

            nearestAddress = address;
            nearestName = name;
        }

        while (targetIndex < targets.Length)
        {
            var target = targets[targetIndex++];
            resolved[target.RelativeAddress] = new ResolvedSymbol(
                nearestName,
                target.AbsoluteAddress - nearestAddress);
        }

        process.WaitForExit();
        var error = errorTask.GetAwaiter().GetResult();
        if (process.ExitCode != 0)
        {
            throw new InvalidOperationException(
                $"nm.exe failed with exit code {process.ExitCode}: {error.Trim()}");
        }

        return new GnuSymbolResolver(resolved);
    }

    public bool TryGet(ulong relativeAddress, out ResolvedSymbol symbol) =>
        symbols.TryGetValue(relativeAddress, out symbol!);

    private static bool TryParseSymbol(string line, out ulong address, out string name)
    {
        address = 0;
        name = string.Empty;
        var firstSpace = line.IndexOf(' ');
        if (firstSpace <= 0 ||
            !ulong.TryParse(
                line.AsSpan(0, firstSpace),
                NumberStyles.HexNumber,
                CultureInfo.InvariantCulture,
                out address))
        {
            return false;
        }

        var typeStart = firstSpace;
        while (typeStart < line.Length && line[typeStart] == ' ')
        {
            typeStart++;
        }

        var nameStart = line.IndexOf(' ', typeStart);
        if (nameStart < 0)
        {
            return false;
        }

        while (nameStart < line.Length && line[nameStart] == ' ')
        {
            nameStart++;
        }

        name = line[nameStart..];
        return name.Length != 0;
    }

    private readonly record struct SymbolTarget(ulong RelativeAddress, ulong AbsoluteAddress);
}

internal sealed record ResolvedSymbol(string Name, ulong Delta);
