"""Local MCP access to saved CreaBIM projects and the CreaBIM CLI."""

import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
from typing import Any

from mcp.server.fastmcp import FastMCP
from mcp.types import ToolAnnotations

import project

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


if os.environ.get('CREABIM_MCP_PORT') and os.environ.get('CREABIM_MCP_TOKEN'):
    def live_call(command: str, args: dict[str, Any], document_id: str | None = None,
                  expected_revision: int | None = None) -> dict[str, Any]:
        token = os.environ['CREABIM_MCP_TOKEN']
        try:
            port = int(os.environ['CREABIM_MCP_PORT'])
            if not 1 <= port <= 65535 or len(token) < 32:
                raise ValueError
        except ValueError:
            raise RuntimeError('Live bridge requires a valid port and a token of at least 32 characters') from None
        request = {'token': token, 'command': command, 'args': args}
        if document_id is not None:
            request['document_id'] = document_id
        if expected_revision is not None:
            request['expected_revision'] = expected_revision
        payload = (json.dumps(request, ensure_ascii=False, allow_nan=False) + '\n').encode('utf-8')
        if len(payload) > 1024 * 1024:
            raise ValueError('Live bridge request exceeds 1 MiB')
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=30) as conn:
                conn.sendall(payload)
                with conn.makefile('rb') as stream:
                    raw = stream.readline(1024 * 1024 + 1)
        except OSError:
            raise RuntimeError('Live bridge connection failed or timed out; inspect the document before retrying') from None
        if not raw.endswith(b'\n') or len(raw) > 1024 * 1024:
            raise RuntimeError('Incomplete or oversized live bridge response; inspect the document before retrying')
        try:
            response = json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError):
            raise RuntimeError('Invalid live bridge response; inspect the document before retrying') from None
        if not isinstance(response, dict) or type(response.get('ok')) is not bool:
            raise RuntimeError('Invalid live bridge response; inspect the document before retrying')
        if not response['ok']:
            raise RuntimeError(str(response.get('error', 'Live bridge rejected the command')).replace(token, '[redacted]'))
        if not isinstance(response.get('result'), dict):
            raise RuntimeError('Invalid live bridge result; inspect the document before retrying')
        return response['result']

    @mcp.tool(annotations=READ_ONLY, structured_output=True)
    def live_inspect() -> dict[str, Any]:
        """Inspect the open document, revision, project path, levels and wall and door types."""
        return live_call('inspect_document', {})

    @mcp.tool(annotations=CREATE, structured_output=True)
    def live_create_wall(document_id: str, expected_revision: int, level_id: str,
                         start: list[float], end: list[float], wall_type_id: str | None = None,
                         thickness: float | None = None, location: str | None = None) -> dict[str, Any]:
        """Create a wall in the open document. Coordinates and thickness are in metres.

        Inspect first for the document ID, revision, level IDs and wall type IDs. A wall
        type uses its defaults and cannot be combined with thickness or location.
        An untyped wall defaults to 0.20 m thickness and Centre location.
        """
        if wall_type_id is not None and (thickness is not None or location is not None):
            raise ValueError('A wall type cannot be combined with thickness or location overrides')
        args = {'level_id': level_id, 'start': start, 'end': end}
        args.update({k: v for k, v in {'wall_type_id': wall_type_id, 'thickness': thickness,
                                       'location': location}.items() if v is not None})
        return live_call('create_wall', args, document_id, expected_revision)

    @mcp.tool(annotations=CREATE, structured_output=True)
    def live_create_door(document_id: str, expected_revision: int, wall_id: str,
                         center: float, opening_type_id: str) -> dict[str, Any]:
        """Place a typed door. Measure the centre from the wall start in metres."""
        args = {'wall_id': wall_id, 'center': center, 'opening_type_id': opening_type_id}
        return live_call('create_door', args, document_id, expected_revision)

    @mcp.tool(annotations=CREATE, structured_output=True)
    def live_update_door(document_id: str, expected_revision: int, element_id: str,
                         material_id: str | None = None,
                         clear_material: bool = False, frame_width: float | None = None,
                         frame_depth: float | None = None, flip_hand: bool | None = None,
                         flip_facing: bool | None = None) -> dict[str, Any]:
        """Edit a door's material, frame, hinge and swing in place.

        Frame dimensions are in metres.
        Set flip_hand to hinge at the end jamb and flip_facing to swing to the exterior.
        Both are states. Use clear_material to remove the assigned material.
        """
        args = {'element_id': element_id}
        args.update({k: v for k, v in {'material_id': material_id, 'frame_width': frame_width,
                                       'frame_depth': frame_depth, 'flip_hand': flip_hand,
                                       'flip_facing': flip_facing}.items() if v is not None})
        if clear_material:
            args['clear_material'] = True
        return live_call('update_door', args, document_id, expected_revision)


if __name__ == '__main__':
    mcp.run(transport='stdio')
