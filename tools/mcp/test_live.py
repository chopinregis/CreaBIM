import asyncio
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

SERVER = Path(__file__).with_name('server.py')
TOKEN = 'a' * 32
SAVED_TOOLS = {'inspect_project', 'list_elements', 'get_element',
               'get_schema', 'check_spec', 'build_project'}


class LiveProtocolTest(unittest.IsolatedAsyncioTestCase):
    async def test_live_tools_are_opt_in_and_dispatch_both_client_contracts(self):
        requests = []

        async def handle(reader, writer):
            request = json.loads(await reader.readline())
            requests.append(request)
            result = {'document_id': 'doc', 'revision': 7, 'levels': [], 'wall_types': []}
            writer.write((json.dumps({'ok': True, 'result': result}) + '\n').encode())
            await writer.drain()
            writer.close()
            await writer.wait_closed()

        document = {'document_id': 'doc'}
        edit = {**document, 'expected_revision': 7}
        wall = {'level_id': 'level', 'start': [0, 0], 'end': [8, 0]}
        slab = {'level_id': 'level', 'boundary': [[0, 0], [3, 0], [0, 3]]}
        typed = {'wall_id': 'host', 'center': 2., 'opening_type_id': 'type'}
        sized = {'wall_id': 'host', 'name': 'Opening', 'offset': 1., 'width': 1.,
                 'height': 1., 'sill': 1.}
        cases = (
            ('inspect', 'inspect_document', {}, {}),
            ('inspect', 'inspect_document', {}, {'offset': 2, 'limit': 4, 'kind': 'Wall'}),
            ('get_element', 'get_element', document, {'element_id': 'beam'}),
            ('create_level', 'create_level', edit, {'name': 'Level', 'elevation': 0., 'height': 3.}),
            ('create_wall', 'create_wall', edit, {**wall, 'wall_type_id': 'type'}),
            ('create_wall', 'create_wall', edit,
             {**wall, 'name': 'Wall', 'height': 2.8, 'thickness': 0.18, 'location': 'Centre'}),
            ('create_door', 'create_door', edit, typed),
            ('create_door', 'create_door', edit, {**sized, 'sill': 0.}),
            ('update_door', 'update_door', edit,
             {'element_id': 'door', 'frame_width': 0.08, 'frame_depth': 0.15,
              'flip_hand': False, 'flip_facing': True, 'clear_material': True}),
            ('create_window', 'create_window', edit, typed),
            ('create_window', 'create_window', edit, sized),
            ('update_window', 'update_window', edit,
             {'element_id': 'window', 'frame_width': 0.05, 'frame_depth': 0.09,
              'panes_across': 3, 'panes_up': 2, 'flip_facing': False}),
            ('create_slab', 'create_slab', edit, {**slab, 'name': 'Slab', 'thickness': 0.15}),
            ('create_slab', 'create_slab', edit, slab),
            ('update_slab', 'update_slab', edit, {'element_id': 'slab', 'top_offset': 0.12}),
            ('create_stair', 'create_stair', edit,
             {'level_id': 'level', 'start': [0, 0], 'toward': [1, 0]}),
            ('update_stair', 'update_stair', edit, {'element_id': 'stair', 'width': 1.4}),
            ('create_railing', 'create_railing', edit,
             {'level_id': 'level', 'path': [[0, 0], [4, 0], [4, 3]]}),
            ('update_railing', 'update_railing', edit, {'element_id': 'rail', 'height': 1.1}),
            ('create_column', 'create_column', edit,
             {'level_id': 'level', 'name': 'Column', 'position': [1, 0], 'width': 0.2,
              'depth': 0.3, 'height': 2.8}),
            ('create_beam', 'create_beam', edit,
             {'level_id': 'level', 'name': 'Beam', 'start': [1, 0], 'end': [3, 0],
              'width': 0.2, 'depth': 0.3, 'top_offset': 0.0}),
            ('move_element', 'move_element', edit, {'element_id': 'beam', 'delta': [1, 2, 0]}),
            ('delete_element', 'delete_element', edit, {'element_id': 'beam'}),
            ('undo', 'undo', edit, {}),
        )
        live_tools = {f'live_{tool}' for tool, _, _, _ in cases}
        self.assertEqual(len(live_tools), 19)

        bridge = await asyncio.start_server(handle, '127.0.0.1', 0)
        try:
            env = {k: v for k, v in os.environ.items()
                   if k not in ('CREABIM_MCP_PORT', 'CREABIM_MCP_TOKEN', 'CREABIM_MCP_TOKEN_FILE')}
            port = str(bridge.sockets[0].getsockname()[1])
            with tempfile.TemporaryDirectory() as tmp:
                token_file = Path(tmp) / 'token'
                token_file.touch(mode=0o600)
                token_file.write_text(TOKEN + '\n', encoding='utf-8')
                setups = [
                    ({}, False, False),
                    ({'CREABIM_MCP_PORT': port}, False, False),
                    ({'CREABIM_MCP_PORT': port, 'CREABIM_MCP_TOKEN': TOKEN}, True, True),
                ]
                if sys.platform in ('darwin', 'linux'):
                    setups.append(({'CREABIM_MCP_PORT': port,
                                    'CREABIM_MCP_TOKEN_FILE': str(token_file)}, True, False))
                for config, enabled, full in setups:
                    params = StdioServerParameters(command=sys.executable, args=[str(SERVER)],
                                                   env={**env, **config})
                    async with stdio_client(params) as (reader, writer):
                        async with ClientSession(reader, writer) as client:
                            await client.initialize()
                            tools = {tool.name: tool for tool in (await client.list_tools()).tools}
                            self.assertEqual(set(tools), SAVED_TOOLS | (live_tools if enabled else set()))
                            if not enabled:
                                continue
                            for tool, command, identifiers, args in (cases if full else cases[:1]):
                                with self.subTest(tool=tool, args=args):
                                    result = await client.call_tool(f'live_{tool}', {**identifiers, **args})
                                    self.assertFalse(result.isError, result)
                                    self.assertEqual(requests[-1],
                                                     {'token': TOKEN, 'command': command,
                                                      'args': args, **identifiers})
                            if not full:
                                continue
                            schema = tools['live_create_wall'].inputSchema
                            self.assertEqual(schema['properties']['start']['items']['type'], 'number')
                            self.assertEqual(
                                {name for name, tool in tools.items() if tool.annotations.destructiveHint},
                                {'live_update_door', 'live_update_window', 'live_update_slab',
                                 'live_update_stair', 'live_update_railing', 'live_move_element',
                                 'live_delete_element', 'live_undo'},
                            )
                            before_rejections = len(requests)
                            for tool, args in (
                                ('wall', {**wall, 'wall_type_id': 'type', 'thickness': 0.2}),
                                ('door', {**typed, 'width': 1.0}),
                                ('window', {'wall_id': 'host', 'center': 2.0}),
                            ):
                                conflict = await client.call_tool(f'live_create_{tool}', {**edit, **args})
                                self.assertTrue(conflict.isError, conflict)
                            self.assertEqual(len(requests), before_rejections)
        finally:
            bridge.close()
            await bridge.wait_closed()
