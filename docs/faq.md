# FAQ

**Does `sc` send my code anywhere?**
No. `sc` itself only sends data for the opt-in LLM review with a remote provider; see [LLM providers](how-to/llm.md). The build and test tools it runs may download your dependencies.

**Does it change my files?**
It writes `.sc/` in the analyzed directory and the file named by `--out`. `sc setup` writes agent config in your home directory; see [agents](how-to/agents.md).

**Why did it pass when a gate shows ✗?**
That gate is advisory. Only the gates in `gates.fail_on` decide the exit status; see [gates](reference/gates.md).

**What is CRAP?**
Complexity weighted by missing test coverage. A complex function with good tests scores low; see [CRAP](crap.md).

**Can I score only my change?**
`--diff BASE` scores what changed against a git ref. `--diff HEAD` covers uncommitted work; see [pre-commit](how-to/pre-commit.md).

**How long does a run take?**
About as long as your test suite, since `sc` runs it. Pack commands share one `--budget-seconds` clock (default 120).

**Which languages?**
Rust, Node, Python, Bash, Go, Java, C#, PHP, C++, and static web pages. See [packs](packs.md).
