import marimo

__generated_with = "0.25.2"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import pandas as pd
    import numpy as np
    import matplotlib.pyplot as plt

    return mo, np, pd, plt


@app.cell
def _(mo, np, plt):
    from pathlib import Path

    # Paths resolve from this notebook, whatever directory marimo runs in.
    EXPERIMENTS = Path(mo.notebook_dir())


    def latest(suite, pattern="run-*"):
        """Newest run directory of an experiment suite that has results."""
        runs = sorted(p.parent for p in (EXPERIMENTS / suite).glob(f"{pattern}/results.csv"))
        assert runs, f"no {pattern}/results.csv under {EXPERIMENTS / suite}"
        return runs[-1]


    def median_iqr(frame, by, value):
        """Median with first and third quartile of `value` per group."""
        return (
            frame.groupby(by)[value]
            .quantile([0.25, 0.5, 0.75])
            .unstack()
            .rename(columns={0.25: "q1", 0.5: "median", 0.75: "q3"})
        )


    def ipc_table(frame, modes):
        """'median [q1, q3]' IPC per benchmark (rows) and mode (columns)."""
        stats = median_iqr(frame, ["benchmark", "mode"], "ipc")
        cells = stats.apply(
            lambda r: f"{r['median']:.3f} [{r['q1']:.3f}, {r['q3']:.3f}]", axis=1
        )
        return cells.unstack()[list(modes)].rename(columns=modes)


    def write_latex(table, path):
        """Write a benchmark-by-mode table as a LaTeX tabular (no jinja2 needed)."""
        rows = [
            r"\begin{tabular}{l|" + "c" * len(table.columns) + "}",
            " & " + " & ".join(table.columns) + r" \\ \hline",
            *(f"\\textbf{{{name}}} & " + " & ".join(row) + r" \\"
              for name, row in table.iterrows()),
            r"\end{tabular}",
        ]
        path.write_text("\n".join(rows) + "\n")


    def normalized_bars(frame, value, baseline, modes, title, ylabel, path):
        """Median of `value` normalized to the baseline mode's median per
        benchmark; error bars span the first to third quartile."""
        base = frame[frame["mode"] == baseline].groupby("benchmark")[value].median()
        normalized = frame.assign(norm=frame[value] / frame["benchmark"].map(base))
        stats = median_iqr(normalized, ["mode", "benchmark"], "norm")
        benchmarks = sorted(base.index)
        x = np.arange(len(benchmarks))
        width = 0.8 / len(modes)

        fig, ax = plt.subplots(figsize=(max(6, 0.45 * len(benchmarks)), 3.8))
        for i, (mode, label) in enumerate(modes.items()):
            rows = stats.loc[mode].reindex(benchmarks)
            ax.bar(x + (i - (len(modes) - 1) / 2) * width, rows["median"], width,
                   yerr=[rows["median"] - rows["q1"], rows["q3"] - rows["median"]],
                   capsize=2, label=label)
        ax.axhline(1.0, color="black", linestyle="--", linewidth=1)
        ax.set_xticks(x, benchmarks, rotation=45, ha="right")
        ax.set_xlabel("Benchmark")
        ax.set_ylabel(ylabel)
        # Title, then the legend on its own row: bars near 1.0 fill the axes,
        # and a long title shares no line with the legend.
        ax.set_title(title, pad=24)
        ax.legend(loc="lower center", bbox_to_anchor=(0.5, 1.0), ncol=len(modes),
                  frameon=False, borderaxespad=0.2)
        ax.grid(axis="y", linestyle="--", alpha=0.4)
        ax.set_axisbelow(True)
        ax.margins(x=0.01)
        fig.tight_layout()
        fig.savefig(path)
        return fig

    return ipc_table, latest, normalized_bars, write_latex


@app.cell(hide_code=True)
def _(mo):
    mo.md("""
    # RISC-V benchmark suite (riscv-tests)

    Median IPC with first and third quartiles, bare metal (M-mode vs. VS-mode
    TVM) and under Linux (native, static TVM, idealized lazy TVM).
    """)
    return


@app.cell
def _(ipc_table, latest, pd, write_latex):
    riscv_dir = latest("riscv-tests")
    riscv = pd.read_csv(riscv_dir / "results.csv")

    bare_metal_ipc = ipc_table(
        riscv[riscv["pair"] == "bare-metal"],
        {"m-mode": "M-mode", "s-mode-tvm": "TVM (VS-mode)"},
    )
    write_latex(bare_metal_ipc, riscv_dir / "bare-metal-ipc.tex")
    bare_metal_ipc
    return riscv, riscv_dir


@app.cell
def _(ipc_table, riscv, riscv_dir, write_latex):
    linux_ipc = ipc_table(
        riscv[riscv["pair"] == "linux"],
        {
            "linux-native": "Linux (native)",
            "linux-tvm-static": "Linux TVM (static)",
            "linux-tvm-lazy": "Linux TVM (idealized lazy)",
        },
    )
    write_latex(linux_ipc, riscv_dir / "linux-ipc.tex")
    linux_ipc
    return


@app.cell
def _(latest, normalized_bars, pd):
    embench_dir = latest("embench")
    normalized_bars(
        pd.read_csv(embench_dir / "results.csv"),
        value="cycles",
        baseline="m-mode",
        modes={"m-mode": "M-mode", "s-mode": "VS-mode"},
        title="Embench benchmarks",
        ylabel="Normalized cycles (median)",
        path=embench_dir / "embench.pdf",
    )
    return


@app.cell
def _(latest, normalized_bars, pd):
    rv8_dir = latest("rv8")
    normalized_bars(
        pd.read_csv(rv8_dir / "results.csv"),
        value="real_seconds",
        baseline="native",
        modes={"native": "native", "tvm": "tvm"},
        title="RV8 benchmarks: native Linux vs. TVM Linux",
        ylabel="Normalized runtime (native = 1.0)",
        path=rv8_dir / "rv8.pdf",
    )
    return


@app.cell
def _(latest, pd):
    sweep = latest("tvm-launch-latency", "sweep-*")
    latency = pd.read_csv(sweep / "results.csv")
    latency["guest_mib"] = latency["guest_ram_size"] // 2**20
    latency["seconds"] = latency["time_ticks"] / 10_000_000  # 10 MHz time CSR

    # Student t for 9 degrees of freedom: every size runs 10 times.
    T95 = 2.262
    assert (latency.groupby("guest_mib")["seq"].nunique() == 10).all()


    def mean_ci(frame, by):
        stats = frame.groupby(by)["seconds"].agg(["mean", "sem"])
        stats["ci"] = T95 * stats["sem"]
        return stats.reset_index()


    def mib_axis(ax, sizes):
        ax.set_xscale("log", base=2)
        ax.set_xticks(sizes, labels=sizes)
        ax.minorticks_off()
        ax.set_xlabel("Guest memory [MiB]")
        ax.grid(which="major", linestyle="--", alpha=0.4)

    return latency, mean_ci, mib_axis, sweep


@app.cell
def _(latency, mean_ci, mib_axis, plt, sweep):
    from matplotlib.ticker import FormatStrFormatter, LogLocator, NullLocator

    _creation = mean_ci(latency[latency["kind"] == "tvm_startup"], "guest_mib")

    _fig, _ax = plt.subplots(figsize=(6, 3.5))
    _ax.errorbar("guest_mib", "mean", yerr="ci", data=_creation, marker="o", capsize=3)
    _ax.set_yscale("log")
    _ax.yaxis.set_major_locator(LogLocator(subs=(1, 2, 5)))
    _ax.yaxis.set_major_formatter(FormatStrFormatter("%g"))
    _ax.yaxis.set_minor_locator(NullLocator())
    mib_axis(_ax, _creation["guest_mib"])
    _ax.set_ylabel("Mean creation time [s]")
    _ax.set_title("TVM creation latency vs. guest size")
    _fig.tight_layout()
    _fig.savefig(sweep / "creation-latency.pdf")
    _fig
    return


@app.cell
def _(latency, mean_ci, mib_axis, plt, sweep):
    # ADD_MEASURED_PAGES runs once per 2 MiB chunk: total each operation per TVM.
    _per_tvm = (
        latency[latency["kind"] == "covh"]
        .groupby(["seq", "operation", "guest_mib"], as_index=False)["seconds"]
        .sum()
    )
    _ops = mean_ci(_per_tvm, ["operation", "guest_mib"])

    _fig, _ax = plt.subplots(figsize=(6, 4))
    for _op, _rows in _ops.groupby("operation"):
        _ax.errorbar("guest_mib", "mean", yerr="ci", data=_rows, marker="o",
                     markersize=4, capsize=3, label=_op)
    _ax.set_yscale("log")
    mib_axis(_ax, sorted(_ops["guest_mib"].unique()))
    _ax.set_ylabel("Mean time per TVM [s]")
    _ax.set_title("CoVE-H operation latency vs. guest size")
    _ax.legend(title="Operation", loc="upper center", bbox_to_anchor=(0.5, -0.2),
               ncol=3, fontsize="small")
    _fig.savefig(sweep / "operation-latency.pdf", bbox_inches="tight")
    _fig
    return


@app.cell
def _():
    return


if __name__ == "__main__":
    app.run()
