import json
import os
from pathlib import Path
import socketserver
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

import live


class Handler(socketserver.StreamRequestHandler):
    def handle(self):
        request = json.loads(self.rfile.readline())
        self.server.requests.append(request)
        if request['command'] == 'bad':
            response = {'ok': False, 'error': 'denied ' + request['token']}
        elif request['command'] == 'malformed':
            self.wfile.write(b'not json\n')
            return
        else:
            response = {'ok': True, 'result': {'command': request['command'], 'args': request['args']}}
        self.wfile.write(json.dumps(response).encode() + b'\n')


class Bridge(socketserver.TCPServer):
    allow_reuse_address = True

    def __init__(self):
        super().__init__(('127.0.0.1', 0), Handler)
        self.requests = []
        self.thread = threading.Thread(target=self.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.shutdown()
        self.server_close()
        self.thread.join()


class LiveTest(unittest.TestCase):
    def test_socket_request_and_redaction(self):
        bridge = Bridge()
        try:
            with patch.dict(os.environ, {'CREABIM_MCP_PORT': str(bridge.server_address[1]),
                                        'CREABIM_MCP_TOKEN': 'private-token-0123456789abcdefghij'}):
                result = live.call('create_wall', {'name': 'Wall', 'height': 2.8}, 'doc-1', 5)
                self.assertEqual(result['args']['height'], 2.8)
                self.assertEqual(bridge.requests[0], {'token': 'private-token-0123456789abcdefghij',
                    'command': 'create_wall', 'args': {'name': 'Wall', 'height': 2.8}, 'document_id': 'doc-1', 'expected_revision': 5})
                with self.assertRaises(ValueError):
                    live.call('create_wall', {}, 'doc-1')
                self.assertEqual(len(bridge.requests), 1)
                with self.assertRaises(RuntimeError) as caught:
                    live.call('bad', {}, 'doc-1', 5)
                self.assertNotIn('private-token-0123456789abcdefghij', str(caught.exception))
                with self.assertRaisesRegex(RuntimeError, 'inspect the active document'):
                    live.call('malformed', {}, 'doc-1', 5)
                with self.assertRaises(ValueError):
                    live.call('large', {'text': 'a' * (1024 * 1024)}, 'doc-1', 5)
        finally:
            bridge.close()

    @unittest.skipUnless(sys.platform in ('darwin', 'linux'), 'private token files are POSIX-only')
    def test_token_file_and_explicit_precedence(self):
        bridge = Bridge()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                token_file = Path(tmp) / 'secret-token-file'
                token_file.write_text('file-secret-value-0123456789abcdef\n', encoding='utf-8')
                token_file.chmod(0o600)
                env = {'CREABIM_MCP_PORT': str(bridge.server_address[1]),
                       'CREABIM_MCP_TOKEN_FILE': str(token_file)}
                with patch.dict(os.environ, env, clear=True):
                    live.call('inspect_document', {})
                    self.assertEqual(bridge.requests[-1]['token'], 'file-secret-value-0123456789abcdef')
                with patch.dict(os.environ, {**env, 'CREABIM_MCP_TOKEN': 'explicit-secret-0123456789abcdefgh',
                                             'CREABIM_MCP_TOKEN_FILE': str(token_file / 'missing')}, clear=True):
                    live.call('inspect_document', {})
                    self.assertEqual(bridge.requests[-1]['token'], 'explicit-secret-0123456789abcdefgh')
                with patch.dict(os.environ, {**env, 'CREABIM_MCP_TOKEN': ''}, clear=True):
                    with self.assertRaises(RuntimeError):
                        live.call('inspect_document', {})
                missing = str(Path(tmp) / 'missing-secret-token-file')
                with patch.dict(os.environ, {**env, 'CREABIM_MCP_TOKEN_FILE': missing}, clear=True):
                    with self.assertRaises(RuntimeError) as caught:
                        live.call('inspect_document', {})
                    self.assertNotIn(missing, str(caught.exception))
                    self.assertNotIn('file-secret-value-0123456789abcdef', str(caught.exception))
                if sys.platform in ('darwin', 'linux'):
                    token_file.chmod(0o644)
                    with patch.dict(os.environ, env, clear=True):
                        with self.assertRaises(RuntimeError) as caught:
                            live.call('inspect_document', {})
                        self.assertNotIn(str(token_file), str(caught.exception))
                    token_file.chmod(0o600)
                    link = Path(tmp) / 'token-link'
                    link.symlink_to(token_file)
                    with patch.dict(os.environ, {**env, 'CREABIM_MCP_TOKEN_FILE': str(link)}, clear=True):
                        with self.assertRaises(RuntimeError):
                            live.call('inspect_document', {})
                    for data in (b'x' * 4097, b'\xff' * 32):
                        token_file.write_bytes(data)
                        with patch.dict(os.environ, env, clear=True):
                            with self.assertRaises(RuntimeError):
                                live.call('inspect_document', {})
                    Path(tmp).chmod(0o755)
                    token_file.write_text('valid-token-0123456789abcdefghi', encoding='utf-8')
                    with patch.dict(os.environ, env, clear=True):
                        with self.assertRaises(RuntimeError):
                            live.call('inspect_document', {})
                    Path(tmp).chmod(0o700)
                with patch('live.sys.platform', 'win32'), patch.dict(os.environ, env, clear=True):
                    with self.assertRaisesRegex(RuntimeError, 'only on macOS and Linux'):
                        live.call('inspect_document', {})
        finally:
            bridge.close()

    def test_missing_configuration_is_clear(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(RuntimeError, 'CREABIM_MCP_PORT'):
                live.call('inspect_document', {})


class LiveMcpTest(unittest.IsolatedAsyncioTestCase):
    async def test_client_uses_live_tools_via_stdio(self):
        bridge = Bridge()
        try:
            server = Path(__file__).with_name('server.py')
            env = {**os.environ, 'CREABIM_MCP_PORT': str(bridge.server_address[1]),
                   'CREABIM_MCP_TOKEN': 'private-token-0123456789abcdefghij'}
            params = StdioServerParameters(command=sys.executable, args=[str(server)], env=env)
            async with stdio_client(params) as (reader, writer):
                async with ClientSession(reader, writer) as client:
                    await client.initialize()
                    tools = {t.name: t for t in (await client.list_tools()).tools}
                    names = set(tools)
                    self.assertTrue({'live_inspect', 'live_create_level', 'live_create_wall',
                                     'live_create_slab', 'live_create_door', 'live_create_window',
                                     'live_move_element', 'live_delete_element', 'live_undo', 'live_get_element',
                                     'live_create_column', 'live_create_beam'} <= names)
                    wall = tools['live_create_wall'].inputSchema['properties']
                    slab = tools['live_create_slab'].inputSchema['properties']
                    move = tools['live_move_element'].inputSchema['properties']
                    self.assertEqual(wall['start']['items']['type'], 'number')
                    self.assertEqual(wall['end']['items']['type'], 'number')
                    self.assertEqual(slab['boundary']['items']['items']['type'], 'number')
                    self.assertEqual(move['delta']['items']['type'], 'number')
                    result = await client.call_tool('live_create_wall', {
                        'document_id': 'doc-1', 'expected_revision': 5, 'level_id': 'test-level', 'name': 'Wall', 'start': [0, 0],
                        'end': [4, 0], 'thickness': 0.2, 'height': 2.8})
                    self.assertFalse(result.isError, result)
                    self.assertEqual(result.structuredContent['command'], 'create_wall')
                    self.assertEqual(result.structuredContent['args']['end'], [4, 0])
                    self.assertEqual(bridge.requests[0]['expected_revision'], 5)
                    result = await client.call_tool('live_get_element', {'document_id': 'doc-1', 'element_id': 'elem-1'})
                    self.assertFalse(result.isError, result)
                    self.assertEqual(result.structuredContent['command'], 'get_element')
                    self.assertEqual(bridge.requests[-1]['document_id'], 'doc-1')
                    result = await client.call_tool('live_inspect', {'offset': 1, 'limit': 5, 'kind': 'Wall'})
                    self.assertFalse(result.isError, result)
                    self.assertEqual(result.structuredContent['command'], 'inspect_document')
                    self.assertEqual(result.structuredContent['args'], {'offset': 1, 'limit': 5, 'kind': 'Wall'})
                    cases = [
                        ('live_create_level', 'create_level', {'name': 'Ground', 'elevation': 0, 'height': 3}),
                        ('live_create_slab', 'create_slab', {'level_id': 'level', 'name': 'Slab',
                            'boundary': [[0, 0], [5, 0], [5, 4]], 'thickness': .2}),
                        ('live_create_door', 'create_door', {'wall_id': 'wall', 'name': 'Door',
                            'offset': 1, 'width': .9, 'height': 2.1, 'sill': 0}),
                        ('live_create_window', 'create_window', {'wall_id': 'wall', 'name': 'Window',
                            'offset': 1, 'width': 1.2, 'height': 1, 'sill': .9}),
                        ('live_create_column', 'create_column', {'level_id': 'level', 'name': 'Column',
                            'position': [2, 3], 'width': .3, 'depth': .4, 'height': 3}),
                        ('live_create_beam', 'create_beam', {'level_id': 'level', 'name': 'Beam',
                            'start': [0, 0], 'end': [4, 0], 'width': .2, 'depth': .4, 'top_offset': .1}),
                        ('live_move_element', 'move_element', {'element_id': 'wall', 'delta': [1, 0, 0]}),
                        ('live_delete_element', 'delete_element', {'element_id': 'wall'}),
                        ('live_undo', 'undo', {}),
                    ]
                    for tool, command, arguments in cases:
                        with self.subTest(tool=tool):
                            result = await client.call_tool(tool, {'document_id': 'doc-1',
                                'expected_revision': 5, **arguments})
                            self.assertFalse(result.isError, result)
                            self.assertEqual(result.structuredContent['command'], command)
                            self.assertEqual(bridge.requests[-1]['document_id'], 'doc-1')
                            self.assertEqual(bridge.requests[-1]['expected_revision'], 5)
        finally:
            bridge.close()


if __name__ == '__main__':
    unittest.main()
