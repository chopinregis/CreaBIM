import asyncio
import json
import os
from pathlib import Path
import sys
import unittest

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

SERVER = Path(__file__).with_name('server.py')
TOKEN = 'a' * 32


class LiveProtocolTest(unittest.IsolatedAsyncioTestCase):
    async def test_live_tools_are_opt_in_and_send_wall_command(self):
        requests = []

        async def handle(reader, writer):
            request = json.loads(await reader.readline())
            requests.append(request)
            result = {'document_id': 'doc', 'revision': 7, 'levels': [], 'wall_types': []}
            writer.write((json.dumps({'ok': True, 'result': result}) + '\n').encode())
            await writer.drain()
            writer.close()
            await writer.wait_closed()

        server = await asyncio.start_server(handle, '127.0.0.1', 0)
        port = server.sockets[0].getsockname()[1]
        try:
            env = {k: v for k, v in os.environ.items()
                   if k not in ('CREABIM_MCP_PORT', 'CREABIM_MCP_TOKEN')}
            for enabled in (False, True):
                live_env = {'CREABIM_MCP_PORT': str(port), 'CREABIM_MCP_TOKEN': TOKEN} if enabled else {}
                params = StdioServerParameters(command=sys.executable, args=[str(SERVER)],
                                               env={**env, **live_env})
                async with stdio_client(params) as (reader, writer):
                    async with ClientSession(reader, writer) as client:
                        await client.initialize()
                        tools = {tool.name: tool for tool in (await client.list_tools()).tools}
                        names = {'inspect_project', 'list_elements', 'get_element',
                                 'get_schema', 'check_spec', 'build_project'}
                        live_names = {'live_inspect', 'live_create_wall', 'live_create_door', 'live_update_door'}
                        self.assertEqual(set(tools), names | (live_names if enabled else set()))
                        if not enabled:
                            continue
                        schema = tools['live_create_wall'].inputSchema
                        self.assertEqual(schema['properties']['start']['items']['type'], 'number')
                        inspect = await client.call_tool('live_inspect', {})
                        self.assertFalse(inspect.isError, inspect)
                        result = await client.call_tool('live_create_wall', {
                            'document_id': 'doc', 'expected_revision': 7, 'level_id': 'level',
                            'start': [0, 0], 'end': [3, 0], 'wall_type_id': 'type'})
                        self.assertFalse(result.isError, result)
                        self.assertEqual(requests[0], {'token': TOKEN, 'command': 'inspect_document', 'args': {}})
                        self.assertEqual(requests[1], {
                            'token': TOKEN, 'command': 'create_wall', 'document_id': 'doc',
                            'expected_revision': 7, 'args': {'level_id': 'level', 'start': [0.0, 0.0],
                                                            'end': [3.0, 0.0], 'wall_type_id': 'type'}})
                        conflict = await client.call_tool('live_create_wall', {
                            'document_id': 'doc', 'expected_revision': 7, 'level_id': 'level',
                            'start': [0, 0], 'end': [3, 0], 'wall_type_id': 'type', 'thickness': 0.2})
                        self.assertTrue(conflict.isError, conflict)
                        self.assertEqual(len(requests), 2)
                        create_schema = tools['live_create_door'].inputSchema
                        self.assertIn('opening_type_id', create_schema['required'])
                        self.assertNotIn('width', create_schema['properties'])
                        edit_schema = tools['live_update_door'].inputSchema
                        self.assertTrue({'width', 'height', 'offset'}.isdisjoint(edit_schema['properties']))
                        for command, args in (
                            ('create_door', {'wall_id': 'host', 'center': 2.0, 'opening_type_id': 'door_type'}),
                            ('update_door', {'element_id': 'door', 'material_id': 'material', 'frame_width': 0.08,
                                             'frame_depth': 0.15, 'flip_hand': False, 'flip_facing': True}),
                        ):
                            result = await client.call_tool(f'live_{command}', {
                                'document_id': 'doc', 'expected_revision': 7, **args})
                            self.assertFalse(result.isError, result)
                            self.assertEqual((requests[-1]['command'], requests[-1]['args']), (command, args))
        finally:
            server.close()
            await server.wait_closed()
