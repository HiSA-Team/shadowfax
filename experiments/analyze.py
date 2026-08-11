import marimo

__generated_with = "0.23.16"
app = marimo.App(width="medium")


@app.cell
def _():
    import marimo as mo
    import pandas as pd

    return (pd,)


@app.cell
def _(pd):
    pd.read_csv("./rv8/run-20260810T144351Z/results.csv")
    return


@app.cell
def _():
    return


if __name__ == "__main__":
    app.run()
