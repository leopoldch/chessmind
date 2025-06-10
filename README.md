# chessmind

Simple chess engine with a Tkinter interface.

## Requirements

Use [Poetry](https://python-poetry.org/) to manage dependencies. Install them with:

```bash
poetry install --no-root
pip install cython numpy
python setup.py build_ext --inplace
```

## Running the GUI

Launch the Tkinter interface with:

```bash
python tk_gui.py
```

At startup you can choose whether to play against another human or the built-in AI engine.

Drag pieces with the mouse. Illegal moves or moves by the wrong color are rejected.
The game ends automatically when a player is checkmated or no legal moves remain.
Pawns promote upon reaching the last rank. The GUI will prompt you to choose the
piece type.

## Playing in the terminal

Run the simple command line interface:

```bash
python play_cli.py
```

## WebSocket interface

Start a simple WebSocket server on `ws://localhost:8765` with:

```bash
python ws_server.py
```

The client should send "white" or "black" to choose the AI color. Then send moves
either in coordinate format like `e2e4` or in standard algebraic notation (e.g.
`Nf3`, `O-O`). The server replies with the engine's move using coordinate
notation.

## Cython acceleration

A small set of Cython extensions speed up move ordering and board evaluation.
After installing dependencies run:

```bash
pip install cython numpy
python setup.py build_ext --inplace
```

This step compiles the modules in `engine_cython/` which the engine uses
automatically if available.

## Profiling

Run `python profile_engine.py` to produce a simple `cProfile` report of the
engine searching for a move from the initial position.

