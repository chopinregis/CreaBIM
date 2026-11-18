# CreaBIM MCP server

A local MCP server for saved CreaBIM projects and model specifications. It runs over stdio, using the official Python MCP SDK and the existing CreaBIM CLI. An optional experimental bridge can inspect the open document and create a wall. No account is needed after setup.

## Setup

From the repository root, build the CLI and install the Python dependency in a virtual environment:

```sh
cargo build --locked -p creabim-cli
python3 -m venv tools/mcp/.venv
tools/mcp/.venv/bin/python -m pip install -r tools/mcp/requirements.txt
```

Python 3.10 or newer is required. On Windows use `tools\mcp\.venv\Scripts\python.exe` instead. The server looks for `creabim` on PATH, then in the repository's `target/release` and `target/debug` folders. To select a particular binary, set `CREABIM_CLI` to its full path. Use a CLI built from a compatible CreaBIM version.

In an MCP host that accepts `mcpServers` JSON, add this entry, replacing both paths with absolute paths on your machine:

```json
{
  "mcpServers": {
    "creabim": {
      "command": "/absolute/path/to/CreaBIM/tools/mcp/.venv/bin/python",
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

The saved-project tools read format-version 1 `.creabim` databases from disk in SQLite read-only mode. They do not support legacy `.creabim.json` or see unsaved changes. They depend on CreaBIM's current SQLite schema. Use a new output path that no other process is creating or opening at the same time; sidecar checks and creation are not one atomic operation. Creating a project requires hard-link support in the destination filesystem; some removable or network volumes do not support it. The host controls access to local paths: only connect trusted clients and review paths before invoking tools.

## Experimental live modeling

This is a separate, opt-in connection to the running application. Standard builds and the six saved-project tools need no flags or network listener. On macOS or Linux, from the repository root, choose an available local port and launch an app built with `live-mcp`:

```sh
export CREABIM_MCP_PORT=62454
export CREABIM_MCP_TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
cargo run --locked -p creabim-app --features live-mcp &
```

Start your MCP host from the same shell so its server inherits both environment variables, and use the configuration above. Keep the token private; do not add it to committed configuration. Both variables must be set when the MCP server starts to expose the live tools. The bridge accepts connections only on `127.0.0.1`; use it only with trusted local clients. Close the app and host to end the session.

- `live_inspect()`: get the open document ID, revision, project path, levels, wall, door and window types. Use the current revision for the next edit.
- `live_create_wall(document_id, expected_revision, level_id, start, end, wall_type_id, thickness, location)`: create a straight wall in the open document. Positions and thickness are in metres. Choose a wall type to use its defaults, or omit it and optionally specify thickness and location; a type cannot be combined with those overrides. Untyped walls default to 0.20 m thickness and Centre location, with the height of the level.
- `live_create_door(document_id, expected_revision, wall_id, center, opening_type_id)`: place a hosted door using an existing door type. Measure `center` along the wall from its start in metres.
- `live_update_door(document_id, expected_revision, element_id, ...)`: edit `frame_width` and `frame_depth` in metres, set `material_id` to an existing material or remove the assignment with `clear_material`. `flip_hand` selects the end jamb when true and the start jamb when false. `flip_facing` selects an exterior swing when true and an interior swing when false. Omitted fields keep their values. Find existing wall, door and material IDs with `list_elements` on a saved copy of the open document.
- `live_create_window(document_id, expected_revision, wall_id, center, opening_type_id)`: place a hosted window with an existing window type. Measure `center` along the wall in metres. The type sets the size and sill.
- `live_update_window(document_id, expected_revision, element_id, ...)`: edit material, frame width and depth, pane counts (`panes_across`, `panes_up`, 1 to 12 each) and inside/outside orientation (`flip_facing`). Frame dimensions are in metres.

For an untyped wall, use `Centre`, `ExteriorFace`, `InteriorFace`, `CoreCentre`, `CoreExterior` or `CoreInterior` as the location. The two input points lie along that line.

Live edits use the application's native undo history. Save in CreaBIM before opening or creating another project. If the document changes, inspect again before editing. A stale revision is rejected. If a connection fails after sending a command, check the model in the app before retrying because the edit may already have completed.

## Test

```sh
tools/mcp/.venv/bin/python -m unittest discover -s tools/mcp -p 'test_*.py' -v
```

The protocol test launches the server with the official MCP client and uses `target/debug/creabim`, so run `cargo build --locked -p creabim-cli` first. It validates and builds `examples/small-house.json` in a temporary directory and checks read-only inspection and overwrite protection.
