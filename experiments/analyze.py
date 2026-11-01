import marimo

__generated_with = "0.24.0"
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

    _frames = [
        pd.read_csv(path)
        for path in sorted(Path("./tvm-launch-latency").glob("run-*/results.csv"))
    ]
    latency = pd.concat([f for f in _frames if "guest_ram_size" in f.columns])
    latency["guest_mib"] = latency["guest_ram_size"] // 2**20
    # QEMU virt clocks the time CSR at 10 MHz.
    latency["seconds"] = latency["time_ticks"] / 10_000_000
    return (latency,)


@app.cell
def _(latency, plt):
    _creation = (
        latency[latency["operation"] == "TVM_STARTUP"]
        .groupby("guest_mib")["seconds"]
        .mean()
    )

    _fig, _ax = plt.subplots()
    _ax.plot(_creation.index, _creation.values, marker="o")
    _ax.set_xticks(_creation.index)
    _ax.set_xlabel("Guest memory [MiB]")
    _ax.set_ylabel("Mean creation time [s]")
    _ax.set_title("TVM creation latency vs. guest size")
    _ax.grid(linestyle=":")
    _fig.tight_layout()
    _fig.savefig("./tvm-launch-latency/creation-latency.pdf")
    _fig
    return


@app.cell
def _(latency, plt):
    _ops = latency[latency["kind"] == "covh"].pivot_table(
        index="guest_mib", columns="operation", values="seconds", aggfunc="mean"
    )

    _fig, _ax = plt.subplots()
    _ops.plot(ax=_ax, marker="o")
    _ax.set_yscale("log")
    _ax.set_xlabel("Guest memory [MiB]")
    _ax.set_ylabel("Mean time [s]")
    _ax.set_title("CoVE-H operation latency vs. guest size")
    _ax.grid(which="both", linestyle=":")
    _ax.legend(
        title="Operation",
        loc="upper center",
        bbox_to_anchor=(0.5, -0.18),
        ncol=3,
        fontsize="small",
    )
    _fig.savefig("./tvm-launch-latency/operation-latency.pdf", bbox_inches="tight")
    _fig
    return


@app.cell
def _():
    return


if __name__ == "__main__":
    app.run()
