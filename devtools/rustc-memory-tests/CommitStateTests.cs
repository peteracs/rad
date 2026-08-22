using Xunit;

public sealed class CommitStateTests
{
    [Fact]
    public void OverlappingCommitCountsOnlyNewPagesAndKeepsOriginalAttribution()
    {
        var state = new CommitState();

        Assert.Equal(100UL, state.Commit(100, 100, 1));
        Assert.Equal(50UL, state.Commit(150, 100, 2));

        Assert.Equal(150UL, state.CommittedBytes);
        Assert.Equal(new StackTotal(100, 1), state.TotalsByStack()[1]);
        Assert.Equal(new StackTotal(50, 1), state.TotalsByStack()[2]);
    }

    [Fact]
    public void PartialDecommitSplitsARegionWithoutLosingAttribution()
    {
        var state = new CommitState();
        state.Commit(100, 100, 1);
        state.Commit(200, 50, 2);

        state.Remove(140, 20);

        Assert.Equal(130UL, state.CommittedBytes);
        Assert.Equal(new StackTotal(80, 2), state.TotalsByStack()[1]);
        Assert.Equal(new StackTotal(50, 1), state.TotalsByStack()[2]);
    }

    [Fact]
    public void RecommitAttributesOnlyTheDecommittedHoleToTheNewStack()
    {
        var state = new CommitState();
        state.Commit(100, 100, 1);
        state.Remove(140, 20);

        Assert.Equal(20UL, state.Commit(140, 20, 3));

        Assert.Equal(100UL, state.CommittedBytes);
        Assert.Equal(new StackTotal(80, 2), state.TotalsByStack()[1]);
        Assert.Equal(new StackTotal(20, 1), state.TotalsByStack()[3]);
    }

    [Fact]
    public void ZeroLengthReleaseUsesTheReservationExtent()
    {
        var state = new CommitState();
        state.Reserve(1_000, 200);
        state.Commit(1_000, 64, 1);
        state.Commit(1_128, 64, 2);

        Assert.True(state.Release(1_000, 0));

        Assert.Equal(0UL, state.CommittedBytes);
        Assert.Empty(state.TotalsByStack());
    }

    [Fact]
    public void UnknownZeroLengthReleaseIsRejectedWithoutChangingState()
    {
        var state = new CommitState();
        state.Commit(4_000, 32, 1);

        Assert.False(state.Release(8_000, 0));

        Assert.Equal(32UL, state.CommittedBytes);
    }

    [Fact]
    public void AddressOverflowIsRejected()
    {
        var state = new CommitState();

        Assert.Throws<OverflowException>(() => state.Commit(ulong.MaxValue - 4, 8, 1));
        Assert.Equal(0UL, state.CommittedBytes);
    }
}
