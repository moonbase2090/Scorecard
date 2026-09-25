using Xunit;

public class AppTest {
    [Fact]
    public void positive() {
        Assert.Equal("pos", App.choose(1));
    }
}
