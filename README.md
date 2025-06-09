# chessmind

Simple chess engine with a Tkinter interface.

## Requirements

Use [Poetry](https://python-poetry.org/) to manage dependencies. Install them with:

```bash
poetry install --no-root
```

## Running the GUI

Launch the Tkinter interface with:

```bash
python tk_gui.py
```

Drag pieces with the mouse. Illegal moves or moves by the wrong color are rejected.
The game ends automatically when a player is checkmated or no legal moves remain.
Pawns promote upon reaching the last rank. The GUI will prompt you to choose the piece type.
