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
                        live = {'live_inspect', 'live_create_wall', 'live_create_railing', 'live_update_railing'}
                        self.assertEqual(set(tools), names | (live if enabled else set()))
                        if not enabled:
                            continue
                        self.assertNotIn('height', tools['live_create_railing'].inputSchema['properties'])
                        self.assertIn('height', tools['live_update_railing'].inputSchema['required'])
                        self.assertNotIn('post_spacing', tools['live_update_railing'].inputSchema['properties'])
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
                        railing = await client.call_tool('live_create_railing', {
                            'document_id': 'doc', 'expected_revision': 7, 'level_id': 'level',
                            'path': [[0, 0], [4, 0], [4, 3]]})
                        self.assertFalse(railing.isError, railing)
                        self.assertEqual(requests[-1]['command'], 'create_railing')
                        edit = await client.call_tool('live_update_railing', {
                            'document_id': 'doc', 'expected_revision': 7, 'element_id': 'rail',
                            'height': 1.1})
                        self.assertFalse(edit.isError, edit)
                        self.assertEqual(requests[-1]['command'], 'update_railing')
                        self.assertEqual(requests[-1]['args'], {
                            'element_id': 'rail', 'height': 1.1})
        finally:
            server.close()
            await server.wait_closed()
