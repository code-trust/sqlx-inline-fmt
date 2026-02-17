# sqlx-inline-fmt

`sqlx-inline-fmt` finds inline `sqlx::query*!` macros in Rust source files,
runs each SQL string through an external formatter, and rewrites the literals
in place.

## Installation

Install from crates.io:

```bash
cargo install sqlx-inline-fmt
```

## Usage

```
sqlx-inline-fmt [OPTIONS] --formatter "<COMMAND>" [INPUTS]...
```

* **INPUTS**: Files or directories to scan. If omitted, the tool scans `./src` and `./tests`.
* **--formatter "\<COMMAND\>" (required)**: The external SQL formatter to run.
  * The command is parsed like a shell string.
  * `sqlx-inline-fmt` will append `-` as the final argument and send the SQL on
    stdin. The formatted SQL should be written to stdout.
* **--check**: Don’t write changes; just report which files would change. Exits
  with code 1 if any file would change (useful for CI).
* **-v, --verbose**: Include `ok` entries in per-file status output.
* **-q, --quiet**: Suppress per-file status lines; only print warnings/errors.

### Examples

```bash
sqlx-inline-fmt --formatter pg_format path/to/file.rs path/to/dir
sqlx-inline-fmt --formatter "sqlfluff fix --dialect postgres"
```
