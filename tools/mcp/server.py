"""Local MCP access to saved CreaBIM projects and the CreaBIM CLI."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from typing import Any

from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

import project
import live

mcp = FastMCP('CreaBIM')
READ_ONLY = ToolAnnotations(readOnlyHint=True, destructiveHint=False, openWorldHint=False)
CREATE = ToolAnnotations(readOnlyHint=False, destructiveHint=False, openWorldHint=False)


def run_cli(*args: str) -> tuple[str, str]:
    root = Path(__file__).resolve().parents[2]
    configured = os.environ.get('CREABIM_CLI')
    binary = 'creabim.exe' if os.name == 'nt' else 'creabim'
    candidates = [configured] if configured else [
        shutil.which(binary), root / 'target' / 'release' / binary,
        root / 'target' / 'debug' / binary,
    ]
    executable = next((str(p) for p in candidates if p and Path(p).is_file()), None)
    if executable is None:
        raise RuntimeError('CreaBIM CLI not found. Set CREABIM_CLI to the executable path.')
    try:
        result = subprocess.run([executable, *args], capture_output=True, text=True,
                                timeout=120, check=False)
    except (OSError, subprocess.TimeoutExpired) as e:
        raise RuntimeError(f'Could not run CreaBIM CLI: {e}') from e
    if result.returncode:
        raise ValueError((result.stderr or result.stdout or 'CreaBIM command failed').strip())
    return result.stdout.strip(), result.stderr.strip()


@mcp.tool(annotations=READ_ONLY, structured_output=True)
def inspect_project(project_path: str) -> dict[str, Any]:
    """Read metadata and element count from a saved .creabim project."""
    return project.inspect_project(project_path)


@mcp.tool(annotations=READ_ONLY, structured_output=True)
def list_elements(project_path: str, kind: str | None = None, offset: int = 0,
                  limit: int = 50, search: str | None = None) -> dict[str, Any]:
    """List saved elements by optional exact kind and name search, in pages of up to 200."""
    return project.list_elements(project_path, kind, offset, limit, search)


@mcp.tool(annotations=READ_ONLY, structured_output=True)
def get_element(project_path: str, element_id: str) -> dict[str, Any]:
    """Get all stored properties of one element by its ID."""
    return project.get_element(project_path, element_id)


@mcp.tool(annotations=READ_ONLY, structured_output=True)
def get_schema() -> dict[str, Any]:
    """Return the JSON schema for CreaBIM model specifications."""
    return json.loads(run_cli('json-schema')[0])


@mcp.tool(annotations=READ_ONLY, structured_output=True)
def check_spec(spec_path: str) -> dict[str, Any]:
    """Check a model specification JSON file without saving a project."""
    output, warnings = run_cli('build', '--check', str(Path(spec_path).expanduser().absolute()))
    return {'valid': True, 'report': output, 'warnings': warnings}


@mcp.tool(annotations=CREATE, structured_output=True)
def build_project(spec_path: str, output_path: str) -> dict[str, Any]:
    """Build a new .creabim project from a spec. Existing files are never replaced."""
    spec = Path(spec_path).expanduser().absolute()
    output = Path(output_path).expanduser().absolute()
    if output.suffix.lower() != '.creabim':
        raise ValueError('Output must have the .creabim extension')
    def check_destination():
        for path in (output, *(Path(str(output) + suffix) for suffix in ('-wal', '-shm', '-journal'))):
            if os.path.lexists(path):
                raise FileExistsError(f'Output or SQLite sidecar already exists: {path}')

    check_destination()
    if not output.parent.is_dir():
        raise ValueError(f'Output directory does not exist: {output.parent}')
    with tempfile.TemporaryDirectory(prefix='.creabim-mcp-', dir=output.parent) as tmp:
        candidate = Path(tmp) / output.name
        report, warnings = run_cli('build', str(spec), str(candidate))
        check_destination()
        # Linking on the same filesystem makes no-overwrite atomic.
        os.link(candidate, output)
    return {'path': str(output), 'report': report, 'warnings': warnings}


@mcp.tool(annotations=READ_ONLY, structured_output=True)
def live_inspect(offset: int = 0, limit: int = 50, kind: str | None = None) -> dict[str, Any]:
    """Inspect the active document and its elements. Retain document_id and revision for edits."""
    args: dict[str, Any] = {'offset': offset, 'limit': limit}
    if kind is not None:
        args['kind'] = kind
    return live.call('inspect_document', args)


@mcp.tool(annotations=READ_ONLY, structured_output=True)
def live_get_element(document_id: str, element_id: str) -> dict[str, Any]:
    """Read one element UUID from the inspected active document."""
    return live.call('get_element', {'element_id': element_id}, document_id)


@mcp.tool(annotations=CREATE, structured_output=True)
def live_create_level(document_id: str, expected_revision: int, name: str, elevation: float, height: float) -> dict[str, Any]:
    """Create a level in the inspected document; elevation and height are in metres."""
    return live.call('create_level', {'name': name, 'elevation': elevation, 'height': height}, document_id, expected_revision)


@mcp.tool(annotations=CREATE, structured_output=True)
def live_create_wall(document_id: str, expected_revision: int, level_id: str, name: str, start: list[float],
                     end: list[float], thickness: float, height: float) -> dict[str, Any]:
    """Create a straight wall on a level UUID; XY coordinates and dimensions are in metres."""
    return live.call('create_wall', {'level_id': level_id, 'name': name, 'start': start,
                                    'end': end, 'thickness': thickness, 'height': height}, document_id, expected_revision)


@mcp.tool(annotations=CREATE, structured_output=True)
def live_create_slab(document_id: str, expected_revision: int, level_id: str, name: str, boundary: list[list[float]],
                     thickness: float) -> dict[str, Any]:
    """Create a slab on a level UUID; the XY boundary and thickness are in metres."""
    return live.call('create_slab', {'level_id': level_id, 'name': name, 'boundary': boundary,
                                    'thickness': thickness}, document_id, expected_revision)


@mcp.tool(annotations=CREATE, structured_output=True)
def live_create_door(document_id: str, expected_revision: int, wall_id: str, name: str, offset: float, width: float,
                     height: float, sill: float) -> dict[str, Any]:
    """Create a door hosted by a wall UUID; offset is from the wall start to the opening near edge. Units: metres."""
    return live.call('create_door', {'wall_id': wall_id, 'name': name, 'offset': offset,
                                    'width': width, 'height': height, 'sill': sill}, document_id, expected_revision)


@mcp.tool(annotations=CREATE, structured_output=True)
def live_create_window(document_id: str, expected_revision: int, wall_id: str, name: str, offset: float, width: float,
                       height: float, sill: float) -> dict[str, Any]:
    """Create a window hosted by a wall UUID; offset is from the wall start to the opening near edge. Units: metres."""
    return live.call('create_window', {'wall_id': wall_id, 'name': name, 'offset': offset,
                                      'width': width, 'height': height, 'sill': sill}, document_id, expected_revision)


@mcp.tool(annotations=CREATE, structured_output=True)
def live_create_column(document_id: str, expected_revision: int, level_id: str, name: str,
                       position: list[float], width: float, depth: float,
                       height: float) -> dict[str, Any]:
    """Create a rectangular column on a level UUID; XY position and dimensions are in metres."""
    return live.call('create_column', {'level_id': level_id, 'name': name, 'position': position,
                                      'width': width, 'depth': depth, 'height': height}, document_id, expected_revision)


@mcp.tool(annotations=CREATE, structured_output=True)
def live_create_beam(document_id: str, expected_revision: int, level_id: str, name: str,
                     start: list[float], end: list[float], width: float,
                     depth: float, top_offset: float = 0) -> dict[str, Any]:
    """Create a rectangular beam on a level UUID; coordinates and dimensions are in metres. top_offset is below the level top."""
    return live.call('create_beam', {'level_id': level_id, 'name': name, 'start': start, 'end': end,
                                    'width': width, 'depth': depth, 'top_offset': top_offset}, document_id, expected_revision)


@mcp.tool(annotations=CREATE, structured_output=True)
def live_move_element(document_id: str, expected_revision: int, element_id: str, delta: list[float]) -> dict[str, Any]:
    """Move a wall, slab, column or beam UUID by an XYZ displacement in metres; openings follow their host wall."""
    return live.call('move_element', {'element_id': element_id, 'delta': delta}, document_id, expected_revision)


@mcp.tool(annotations=ToolAnnotations(readOnlyHint=False, destructiveHint=True, openWorldHint=False), structured_output=True)
def live_delete_element(document_id: str, expected_revision: int, element_id: str) -> dict[str, Any]:
    """Delete a wall, opening, column or beam UUID; refuses deletion when dependent elements exist."""
    return live.call('delete_element', {'element_id': element_id}, document_id, expected_revision)


@mcp.tool(annotations=ToolAnnotations(readOnlyHint=False, destructiveHint=True, openWorldHint=False), structured_output=True)
def live_undo(document_id: str, expected_revision: int) -> dict[str, Any]:
    """Undo the most recent edit of the inspected document, whether made by a user or agent."""
    return live.call('undo', {}, document_id, expected_revision)


if __name__ == '__main__':
    mcp.run(transport='stdio')
