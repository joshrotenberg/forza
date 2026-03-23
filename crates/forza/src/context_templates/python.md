# Python project context

## Language and toolchain

This is a Python project. Check `pyproject.toml`, `setup.py`, or `requirements.txt` to understand how the project is configured and which tools it uses.

## Common commands

```bash
# With pip
pip install -e ".[dev]"       # install in editable mode with dev extras
pytest                         # run tests
ruff check .                   # lint (if ruff is used)
ruff format .                  # format (if ruff is used)
mypy .                         # type check (if mypy is used)

# With poetry
poetry install                 # install dependencies
poetry run pytest              # run tests
poetry run ruff check .        # lint

# With uv
uv sync                        # install dependencies
uv run pytest                  # run tests
```

Check `pyproject.toml` `[tool.pytest.ini_options]`, `[tool.ruff]`, and `[tool.mypy]` for project-specific configuration.

## Conventions

- Follow PEP 8 and the existing style in the codebase.
- Add type hints to new functions and methods.
- Write tests with `pytest`; use fixtures for shared setup.
- Do not commit `.pyc` files or `__pycache__/` directories.
- Check `.github/workflows/` for the exact test and lint commands used in CI.

## Project layout

Check `pyproject.toml` for package name, version, and dependencies.
