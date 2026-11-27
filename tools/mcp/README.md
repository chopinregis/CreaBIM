# CreaBIM MCP server

A local MCP server for saved CreaBIM projects and model specifications. It runs over stdio, using the official Python MCP SDK and the existing CreaBIM CLI. An optional bridge can inspect and edit the open document. No account is needed after setup.

The [macOS MCP Beta](../../packaging/macos/MCP_BETA_README.txt) packages the
server and CLI together and provides a separate app with opt-in live editing.

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

## Live tools

This is a separate, opt-in connection to the running application. Standard builds and the six saved-project tools need no flags or network listener. On macOS or Linux, from the repository root, choose an available local port and launch an app built with `live-mcp`:

```sh
export CREABIM_MCP_PORT=62454
export CREABIM_MCP_TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
cargo run --locked -p creabim-app --features live-mcp &
```

Start your MCP host from the same shell so its server inherits the environment variables. The Python server also accepts `CREABIM_MCP_TOKEN_FILE` on macOS and Linux in place of `CREABIM_MCP_TOKEN`. Use an owner-only regular file with mode `0600` in an owner-only directory with mode `0700`. The app bridge takes the literal token through `CREABIM_MCP_TOKEN`. The bridge binds to `127.0.0.1`. Keep the token private and connect only trusted local clients.

`live_inspect(offset, limit, kind)` returns the document ID, revision, project path, levels, wall and opening types, and a page of elements. All three filters are optional. `live_create_wall` takes `level_id`, `start` and `end`. Optional `name` and `height` customize the wall. Specify `wall_type_id` to use its thickness and location, or omit it and set `thickness` and `location` for an untyped wall. Untyped defaults are 0.20 m and `Centre`. Location also accepts `ExteriorFace`, `InteriorFace`, `CoreCentre`, `CoreExterior` and `CoreInterior`.

### Doors, windows, slabs, stairs and railings

- `live_create_door` and `live_create_window` take a host `wall_id`. For a typed opening, pass `center` measured from the wall start and `opening_type_id`. For explicit dimensions, pass `name`, `offset` to the near edge from the wall start, `width`, `height` and `sill`. `live_update_door` and `live_update_window` expose material, frame and orientation settings, plus the pane grid for windows.
- `live_create_slab` takes `level_id` and an XY `boundary`, with optional `name` and `thickness`. `live_update_slab` can set thickness and top offset.
- `live_create_stair` takes `level_id`, `start` and a `toward` point indicating the direction. `live_update_stair` sets the width. `live_create_railing` takes `level_id` and a `path`. `live_update_railing` sets the height.

### Levels and structure

- `live_create_level` creates a level with a name, elevation and height.
- `live_create_column` and `live_create_beam` create native structural elements on a level.

### Element queries and edits

- `live_get_element(document_id, element_id)` reads one element from the open document.
- `live_move_element` applies an XYZ displacement to a wall, slab, column or beam.
- `live_delete_element` deletes supported elements when they have no dependent elements.
- `live_undo` undoes the most recent edit, including edits made in the app.

All coordinates and lengths are in metres. Each edit requires the inspected `document_id` and current `expected_revision`. Use the returned revision for the next edit, or inspect again if the active project changed. Edits follow the active modeling pane's phase and option, and use the application's native undo history. Save through CreaBIM to make changes persistent. If a connection fails after sending an edit, inspect before retrying because the edit may have completed.

## Test

```sh
tools/mcp/.venv/bin/python -m unittest discover -s tools/mcp -p 'test_*.py' -v
```

The protocol test launches the server with the official MCP client and uses `target/debug/creabim` unless `CREABIM_CLI` selects a different build. Build the CLI first. It validates and builds `examples/small-house.json` in a temporary directory and checks read-only inspection and overwrite protection.
