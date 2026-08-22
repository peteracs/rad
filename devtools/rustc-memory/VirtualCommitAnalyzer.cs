using System.Text;
using Microsoft.Diagnostics.Tracing.Etlx;
using Microsoft.Diagnostics.Tracing.Parsers.Kernel;

internal sealed class VirtualCommitAnalyzer
{
    private readonly List<StackInfo> stacks;
    private readonly Dictionary<int, StackTotal> peakByStack;

    private VirtualCommitAnalyzer(
        TraceProcess process,
        List<StackInfo> stacks,
        Dictionary<int, StackTotal> peakByStack,
        long commitEvents,
        ulong cumulativeCommittedBytes,
        ulong peakCommittedBytes,
        double peakTimeMilliseconds,
        ulong finalCommittedBytes,
        long unresolvedReleases)
    {
        Process = process;
        this.stacks = stacks;
        this.peakByStack = peakByStack;
        CommitEvents = commitEvents;
        CumulativeCommittedBytes = cumulativeCommittedBytes;
        PeakCommittedBytes = peakCommittedBytes;
        PeakTimeMilliseconds = peakTimeMilliseconds;
        FinalCommittedBytes = finalCommittedBytes;
        UnresolvedReleases = unresolvedReleases;
    }

    public TraceProcess Process { get; }
    public long CommitEvents { get; }
    public ulong CumulativeCommittedBytes { get; }
    public ulong PeakCommittedBytes { get; }
    public double PeakTimeMilliseconds { get; }
    public ulong FinalCommittedBytes { get; }
    public long UnresolvedReleases { get; }

    public static VirtualCommitAnalyzer Analyze(TraceProcess process)
    {
        var state = new CommitState();
        var stackRegistry = new StackRegistry();
        long commitEvents = 0;
        long unresolvedReleases = 0;
        ulong cumulativeCommittedBytes = 0;
        ulong peakCommittedBytes = 0;
        var peakTimeMilliseconds = 0.0;
        var peakByStack = new Dictionary<int, StackTotal>();

        foreach (var allocation in process.EventsInProcess.ByEventType<VirtualAllocTraceData>())
        {
            if (allocation.Length < 0)
            {
                throw new InvalidDataException(
                    $"negative VirtualAlloc length at {allocation.TimeStampRelativeMSec:F3} ms");
            }

            var start = allocation.BaseAddr;
            var length = (ulong)allocation.Length;
            var flags = allocation.Flags;

            if ((flags & VirtualAllocTraceData.VirtualAllocFlags.MEM_RESERVE) != 0 && length != 0)
            {
                state.Reserve(start, length);
            }

            if ((flags & VirtualAllocTraceData.VirtualAllocFlags.MEM_COMMIT) != 0 && length != 0)
            {
                commitEvents++;
                var stackId = stackRegistry.Intern(allocation.CallStack());
                cumulativeCommittedBytes += state.Commit(start, length, stackId);
            }

            if ((flags & VirtualAllocTraceData.VirtualAllocFlags.MEM_DECOMMIT) != 0 && length != 0)
            {
                state.Remove(start, length);
            }

            if ((flags & VirtualAllocTraceData.VirtualAllocFlags.MEM_RELEASE) != 0)
            {
                if (!state.Release(start, length))
                {
                    unresolvedReleases++;
                }
            }

            if (state.CommittedBytes > peakCommittedBytes)
            {
                peakCommittedBytes = state.CommittedBytes;
                peakTimeMilliseconds = allocation.TimeStampRelativeMSec;
                peakByStack = state.TotalsByStack();
            }
        }

        return new VirtualCommitAnalyzer(
            process,
            stackRegistry.Stacks,
            peakByStack,
            commitEvents,
            cumulativeCommittedBytes,
            peakCommittedBytes,
            peakTimeMilliseconds,
            state.CommittedBytes,
            unresolvedReleases);
    }

    public string? FindRustcDriverPath() => stacks
        .SelectMany(stack => stack.Frames)
        .Where(frame => frame.ModuleName.StartsWith("rustc_driver", StringComparison.Ordinal))
        .Select(frame => frame.ModulePath)
        .FirstOrDefault(File.Exists);

    public IReadOnlyCollection<ulong> RustcDriverRvasAtPeak() => RankedStacks()
        .SelectMany(total => stacks[total.StackId].Frames)
        .Where(frame => frame.ModuleName.StartsWith("rustc_driver", StringComparison.Ordinal))
        .Select(frame => frame.RelativeAddress)
        .Distinct()
        .ToArray();

    public void WriteReport(
        TextWriter writer,
        GnuSymbolResolver symbols,
        int stackLimit,
        int frameLimit)
    {
        var accountedBytes = peakByStack.Values.Aggregate(0UL, (sum, total) => sum + total.Bytes);
        writer.WriteLine($"process={Process.Name} pid={Process.ProcessID}");
        writer.WriteLine($"commit_events={CommitEvents}");
        writer.WriteLine($"cumulative_committed_mib={ToMib(CumulativeCommittedBytes):F3}");
        writer.WriteLine($"peak_committed_mib={ToMib(PeakCommittedBytes):F3}");
        writer.WriteLine($"accounted_peak_mib={ToMib(accountedBytes):F3}");
        writer.WriteLine($"peak_allocation_stacks={peakByStack.Count}");
        writer.WriteLine($"peak_time_ms={PeakTimeMilliseconds:F3}");
        writer.WriteLine($"final_committed_mib={ToMib(FinalCommittedBytes):F3}");
        writer.WriteLine($"unresolved_releases={UnresolvedReleases}");

        writer.WriteLine();
        writer.WriteLine("inclusive compiler frames at peak (a stack contributes once per symbol):");
        var frameRank = 0;
        foreach (var frame in InclusiveCompilerFrames(symbols).Take(40))
        {
            frameRank++;
            writer.WriteLine(
                $"@{frameRank} peak_live_mib={ToMib(frame.Bytes):F3} " +
                $"allocation_stacks={frame.Stacks} {Limit(frame.Symbol, 260)}");
        }

        var rank = 0;
        foreach (var total in RankedStacks().Take(stackLimit))
        {
            rank++;
            var stack = stacks[total.StackId];
            writer.WriteLine();
            writer.WriteLine(
                $"#{rank} peak_live_mib={ToMib(total.Bytes):F3} " +
                $"segments={total.Segments} stack_id={total.StackId}");

            var writtenFrames = 0;
            foreach (var frame in stack.Frames.Where(IsRelevantCompilerFrame))
            {
                if (writtenFrames == 40)
                {
                    writer.WriteLine("  ... remaining compiler frames omitted");
                    break;
                }

                writer.Write("  ");
                writer.Write(frame.ModuleName);
                writer.Write($"+0x{frame.RelativeAddress:x}");
                if (symbols.TryGet(frame.RelativeAddress, out var symbol))
                {
                    writer.Write($" {Limit(symbol.Name, 260)}+0x{symbol.Delta:x}");
                }

                writer.WriteLine();
                writtenFrames++;
                if (writtenFrames == frameLimit)
                {
                    writer.WriteLine("  ... remaining compiler frames omitted");
                    break;
                }
            }
        }
    }

    private IEnumerable<InclusiveFrame> InclusiveCompilerFrames(GnuSymbolResolver symbols)
    {
        var totals = new Dictionary<string, InclusiveFrame>(StringComparer.Ordinal);
        foreach (var stackTotal in RankedStacks())
        {
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var frame in stacks[stackTotal.StackId].Frames.Where(IsRelevantCompilerFrame))
            {
                var symbol = symbols.TryGet(frame.RelativeAddress, out var resolved)
                    ? resolved.Name
                    : $"{frame.ModuleName}+0x{frame.RelativeAddress:x}";
                if (!seen.Add(symbol))
                {
                    continue;
                }

                if (totals.TryGetValue(symbol, out var total))
                {
                    totals[symbol] = total with
                    {
                        Bytes = total.Bytes + stackTotal.Bytes,
                        Stacks = total.Stacks + 1,
                    };
                }
                else
                {
                    totals.Add(symbol, new InclusiveFrame(symbol, stackTotal.Bytes, 1));
                }
            }
        }

        return totals.Values
            .OrderByDescending(total => total.Bytes)
            .ThenBy(total => total.Symbol, StringComparer.Ordinal);
    }

    private IEnumerable<RankedStack> RankedStacks() => peakByStack
        .Select(pair => new RankedStack(pair.Key, pair.Value.Bytes, pair.Value.Segments))
        .OrderByDescending(total => total.Bytes)
        .ThenBy(total => total.StackId);

    private static bool IsRelevantCompilerFrame(FrameInfo frame) =>
        frame.ModuleName.StartsWith("rustc_driver", StringComparison.Ordinal);

    private static double ToMib(ulong bytes) => bytes / 1024.0 / 1024.0;

    private static string Limit(string value, int length) =>
        value.Length <= length ? value : value[..(length - 3)] + "...";

    private readonly record struct RankedStack(int StackId, ulong Bytes, int Segments);
    private readonly record struct InclusiveFrame(string Symbol, ulong Bytes, int Stacks);
}

internal sealed class CommitState
{
    private static readonly Segment Minimum = new(0, 0, -1);
    private readonly SortedSet<Segment> segments = new(SegmentStartComparer.Instance);
    private readonly Dictionary<ulong, ulong> reservations = new();

    public ulong CommittedBytes { get; private set; }

    public void Reserve(ulong start, ulong length)
    {
        var end = CheckedEnd(start, length);
        reservations[start] = end;
    }

    public ulong Commit(ulong start, ulong length, int stackId)
    {
        var end = CheckedEnd(start, length);
        var overlaps = FindOverlaps(start, end);
        var cursor = start;
        ulong addedBytes = 0;

        foreach (var overlap in overlaps)
        {
            if (cursor < overlap.Start)
            {
                addedBytes += Add(cursor, Math.Min(overlap.Start, end), stackId);
            }

            cursor = Math.Max(cursor, overlap.End);
            if (cursor >= end)
            {
                break;
            }
        }

        if (cursor < end)
        {
            addedBytes += Add(cursor, end, stackId);
        }

        return addedBytes;
    }

    public void Remove(ulong start, ulong length)
    {
        var end = CheckedEnd(start, length);
        var overlaps = FindOverlaps(start, end);

        foreach (var overlap in overlaps)
        {
            segments.Remove(overlap);
            var removedStart = Math.Max(start, overlap.Start);
            var removedEnd = Math.Min(end, overlap.End);
            CommittedBytes -= removedEnd - removedStart;

            if (overlap.Start < start)
            {
                segments.Add(overlap with { End = start });
            }

            if (overlap.End > end)
            {
                segments.Add(overlap with { Start = end });
            }
        }
    }

    public bool Release(ulong start, ulong length)
    {
        ulong end;
        if (length == 0)
        {
            if (!reservations.Remove(start, out end))
            {
                return false;
            }
        }
        else
        {
            end = CheckedEnd(start, length);
            reservations.Remove(start);
        }

        Remove(start, end - start);
        return true;
    }

    public Dictionary<int, StackTotal> TotalsByStack()
    {
        var totals = new Dictionary<int, StackTotal>();
        foreach (var segment in segments)
        {
            var bytes = segment.End - segment.Start;
            if (totals.TryGetValue(segment.StackId, out var total))
            {
                totals[segment.StackId] = total with
                {
                    Bytes = total.Bytes + bytes,
                    Segments = total.Segments + 1,
                };
            }
            else
            {
                totals.Add(segment.StackId, new StackTotal(bytes, 1));
            }
        }

        return totals;
    }

    private ulong Add(ulong start, ulong end, int stackId)
    {
        if (start == end)
        {
            return 0;
        }

        var segment = new Segment(start, end, stackId);
        if (!segments.Add(segment))
        {
            throw new InvalidDataException($"duplicate committed segment start 0x{start:x}");
        }

        var bytes = end - start;
        CommittedBytes += bytes;
        return bytes;
    }

    private List<Segment> FindOverlaps(ulong start, ulong end)
    {
        var overlaps = new List<Segment>();
        var predecessors = segments.GetViewBetween(Minimum, new Segment(start, start, -1));
        if (predecessors.Count != 0)
        {
            var predecessor = predecessors.Max!;
            if (predecessor.Start < start && predecessor.End > start)
            {
                overlaps.Add(predecessor);
            }
        }

        if (start < end)
        {
            foreach (var segment in segments.GetViewBetween(
                         new Segment(start, start, -1),
                         new Segment(end - 1, end - 1, -1)))
            {
                overlaps.Add(segment);
            }
        }

        return overlaps;
    }

    private static ulong CheckedEnd(ulong start, ulong length) =>
        checked(start + length);

    private sealed class SegmentStartComparer : IComparer<Segment>
    {
        public static SegmentStartComparer Instance { get; } = new();

        public int Compare(Segment? left, Segment? right) =>
            left!.Start.CompareTo(right!.Start);
    }
}

internal sealed class StackRegistry
{
    private readonly Dictionary<string, int> idsByKey = new(StringComparer.Ordinal);

    public List<StackInfo> Stacks { get; } = [];

    public int Intern(TraceCallStack? callStack)
    {
        var allUserFrames = new List<FrameInfo>();
        var compilerFrames = new List<FrameInfo>();

        for (var frame = callStack; frame is not null; frame = frame.Caller)
        {
            var codeAddress = frame.CodeAddress;
            if (codeAddress.Address >= 0xffff_0000_0000_0000)
            {
                continue;
            }

            var module = codeAddress.ModuleFile;
            var moduleName = module?.Name ?? "<unknown>";
            var relativeAddress = module is null
                ? codeAddress.Address
                : codeAddress.Address - module.ImageBase;
            var captured = new FrameInfo(
                moduleName,
                module?.FilePath ?? string.Empty,
                relativeAddress);
            allUserFrames.Add(captured);
            if (moduleName.StartsWith("rustc_driver", StringComparison.Ordinal))
            {
                compilerFrames.Add(captured);
            }
        }

        var selectedFrames = compilerFrames.Count == 0 ? allUserFrames : compilerFrames;
        var keyBuilder = new StringBuilder(selectedFrames.Count * 24);
        foreach (var frame in selectedFrames)
        {
            keyBuilder.Append(frame.ModuleName);
            keyBuilder.Append(':');
            keyBuilder.Append(frame.RelativeAddress.ToString("x"));
            keyBuilder.Append(';');
        }

        var key = keyBuilder.ToString();
        if (idsByKey.TryGetValue(key, out var existing))
        {
            return existing;
        }

        var id = Stacks.Count;
        idsByKey.Add(key, id);
        Stacks.Add(new StackInfo(id, selectedFrames.ToArray()));
        return id;
    }
}

internal sealed record Segment(ulong Start, ulong End, int StackId);
internal sealed record StackInfo(int Id, IReadOnlyList<FrameInfo> Frames);
internal sealed record FrameInfo(string ModuleName, string ModulePath, ulong RelativeAddress);
internal readonly record struct StackTotal(ulong Bytes, int Segments);
