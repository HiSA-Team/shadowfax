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


@app.cell(hide_code=True)
def _(benchmarks, mo):
    mo.md(f"""
    # RV8 benchmark analysis

    RV8 is made up by {len(benchmarks)}. They are mixed between compute bound and I/O bound and are executed on Linux native (no virtualization) and a Linux TVM guest.

    They are: {", ".join(benchmarks)}
    """)
    return


@app.cell
def _(pd):
    df = pd.read_csv("./rv8/run-20260817T143333Z/results.csv")
    df = (
        df.drop(columns=["exit_code", "run"])
          .groupby(["mode", "benchmark"], as_index=False)
          .mean()
    )

    native_runtime = (
        df[df["mode"] == "native"]
        .set_index("benchmark")["real_seconds"]
    )

    df["normalized_runtime"] = (
        df["real_seconds"]
        / df["benchmark"].map(native_runtime)
    )
    df
    return (df,)


@app.cell
def _(df, np, plt):
    pivot = df.pivot(
        index="benchmark",
        columns="mode",
        values="normalized_runtime",
    )

    x = np.arange(len(pivot.index))

    width = 0.3
    separation = 0.2

    fig, ax = plt.subplots()

    ax.bar(
        x - separation,
        pivot["native"],
        width,
        label="native",
    )

    ax.bar(
        x + separation,
        pivot["tvm"],
        width,
        label="tvm",
    )

    ax.set_xticks(x)
    ax.set_xticklabels(pivot.index)
    ax.set_ylabel("Normalized runtime (native = 1.0)")
    ax.set_xlabel("Benchmark")
    plt.xticks(rotation=45, ha="right")
    ax.axhline(1.0, color="black", linestyle="--", linewidth=1)
    ax.legend()
    plt.grid()

    plt.tight_layout()
    plt.savefig("./rv8/run-20260817T143333Z/rv8.pdf")
    plt.show()
    return


@app.cell
def _(pd):
    from pathlib import Path

    sweep = sorted(Path("./experiments/tvm-launch-latency").glob("sweep-*"))[-1]
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
    _ops = mean_ci(latency[latency["kind"] == "covh"], ["operation", "guest_mib"])

    _fig, _ax = plt.subplots(figsize=(6, 4))
    for _op, _rows in _ops.groupby("operation"):
        _ax.errorbar("guest_mib", "mean", yerr="ci", data=_rows, marker="o",
                     markersize=4, capsize=3, label=_op)
    _ax.set_yscale("log")
    mib_axis(_ax, sorted(_ops["guest_mib"].unique()))
    _ax.set_ylabel("Mean time [s]")
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
