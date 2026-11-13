# CreaBIM MCP prototype

A local MCP server for saved CreaBIM projects and model specifications. It runs over stdio, using the official Python MCP SDK and the existing CreaBIM CLI. No network service or account is needed after setup.

## Setup

From the repository root, build the CLI and install the Python dependency in a virtual environment:

```sh
cargo build --locked -p creabim-cli
python3 -m venv .venv
.venv/bin/python -m pip install -r tools/mcp/requirements.txt
```

Python 3.10 or newer is required. On Windows use `.venv\\Scripts\\python.exe` instead. The server looks for `creabim` on PATH, then in the repository's `target/release` and `target/debug` folders. To select a particular binary, set `CREABIM_CLI` to its full path. Use a CLI built from a compatible CreaBIM version.

In an MCP host that accepts `mcpServers` JSON, add this entry, replacing both paths with absolute paths on your machine:

```json
{
  "mcpServers": {
    "creabim": {
      "command": "/absolute/path/to/CreaBIM/.venv/bin/python",
      "args": ["/absolute/path/to/CreaBIM/tools/mcp/server.py"],
      "env": {"CREABIM_CLI": "/absolute/path/to/CreaBIM/target/debug/creabim"}
    }
  }
}
```

The host launches the server; it does not print a URL. For a first check, ask the client to validate `examples/small-house.json`, build it to a **new** `.creabim` file, list its `Wall` elements, and retrieve one by ID. The schema tool describes the supported JSON specification.

## Tools

- `inspect_project(project_path)`: saved project metadata and element count.
- `list_elements(project_path, kind, offset, limit, search)`: paginated IDs, kinds and names. Filters are optional; `kind` is exact, and `search` matches part of a name. SQLite ignores case for ASCII letters only.
- `get_element(project_path, element_id)`: complete stored JSON properties for one element; lengths follow the model's units (normally metres).
- `get_schema()`: JSON schema for a model specification.
- `check_spec(spec_path)`: validate a specification and return any CLI warnings.
- `build_project(spec_path, output_path)`: build a new `.creabim` file and return any warnings. It refuses existing files and SQLite sidecars; it does not overwrite them.

This prototype reads format-version 1 `.creabim` databases from disk in SQLite read-only mode. It does not support legacy `.creabim.json` for inspection or control the open CreaBIM window, and it cannot see changes that have not been saved. It depends on CreaBIM's current SQLite schema. Use a new output path that no other process is creating or opening at the same time; sidecar checks and creation are not one atomic operation. Creating a project requires hard-link support in the destination filesystem; some removable or network volumes do not support it. The host controls access to local paths: only connect trusted clients and review paths before invoking tools.

## Test

```sh
.venv/bin/python -m unittest discover -s tools/mcp -p 'test_*.py' -v
```

The protocol test launches the server with the official MCP client and uses `target/debug/creabim`, so run `cargo build --locked -p creabim-cli` first. It validates and builds `examples/small-house.json` in a temporary directory and checks read-only inspection and overwrite protection.

## Optional live modelling prototype

The desktop application in this branch has an opt-in local bridge. A stock CreaBIM release does not contain it. Build and run this branch's `creabim-app`, and set `CREABIM_MCP_PORT` plus either `CREABIM_MCP_TOKEN` or `CREABIM_MCP_TOKEN_FILE` for both the app and the MCP server. The bridge listens only on `127.0.0.1` and is disabled when the port is not set. Use a random token of at least 32 characters. On macOS/Linux the token file must be a regular file owned by the current user, mode `0600`, in a private directory owned by the user, mode `0700`. It must contain UTF-8 text; trailing newlines are ignored. The prototype checks POSIX mode bits but not ACLs, so ensure no ACL grants others access. Token files are not supported on Windows in this prototype; use the explicit environment variable there. Never share the token.

For example, after building the app, a macOS/Linux terminal can generate private credentials in a directory of your choice and start the app:

```sh
mkdir -p /absolute/path/to/private-runtime
chmod 700 /absolute/path/to/private-runtime
umask 077
python3 -c 'import secrets; print(secrets.token_urlsafe(48))' > /absolute/path/to/private-runtime/token
export CREABIM_MCP_PORT=48764
export CREABIM_MCP_TOKEN_FILE=/absolute/path/to/private-runtime/token
cargo run --locked -p creabim-app
```

Set the same two environment variables in the MCP host's server configuration; use a free local port. For a macOS `.app` started via Launch Services, pass them with `open -n --env CREABIM_MCP_PORT --env CREABIM_MCP_TOKEN_FILE /path/to/CreaBIM.app`. Run the host/server with the same values. The host starts the Python MCP process separately.

Call `live_inspect` first. It returns the active `document_id`, model `revision`, path, unsaved flag and a page of elements. To read an element use `live_get_element` with its UUID. Every edit requires the same `document_id` and the current `expected_revision`; take the new revision from the previous response, or inspect again after a user edit. Opening another document changes the document ID. This protects against editing a different project or a stale revision.

Live tools create levels, straight walls, slabs, hosted doors and windows, rectangular columns and beams; move walls, slabs, columns and beams; delete walls, openings, columns and beams when they have no dependents; and undo the last user or agent edit. Inputs use metres (including coordinates), not millimetres. Door/window `offset` is measured along the wall from its start to the near edge. Slabs accept at most 1,000 boundary vertices. Each successful operation is one native CreaBIM undo step. Wall joins use CreaBIM's existing join-following behaviour, including its limit when a followed wall would become shorter than 0.05 m.

The app rejects edits while a drawing, transform or pointer interaction is underway. Changes appear in the open model and remain unsaved until the user saves normally in CreaBIM. Save before opening or creating another project: this CreaBIM version may replace a project without prompting about unsaved changes. A timeout after a command has started can leave its outcome unknown: inspect the current model before retrying. The connection is for trusted local clients, not network exposure. This is a limited prototype, not parity with the full BIMCraft command set.
