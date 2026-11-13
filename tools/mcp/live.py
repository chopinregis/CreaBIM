"""Client for the optional, authenticated local CreaBIM live bridge."""

import json
import os
import socket
import stat
import sys

MAX_LINE = 1024 * 1024
TIMEOUT = 30


def _token():
    if 'CREABIM_MCP_TOKEN' in os.environ:
        token = os.environ['CREABIM_MCP_TOKEN']
    else:
        if sys.platform not in ('darwin', 'linux'):
            raise RuntimeError('Token files are supported only on macOS and Linux; set CREABIM_MCP_TOKEN')
        path = os.environ.get('CREABIM_MCP_TOKEN_FILE', '')
        fd = -1
        try:
            parent = os.stat(os.path.realpath(os.path.dirname(os.path.abspath(path))))
            if (not stat.S_ISDIR(parent.st_mode) or parent.st_uid != os.geteuid()
                    or stat.S_IMODE(parent.st_mode) != 0o700):
                raise ValueError
            before = os.lstat(path)
            fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | getattr(os, 'O_NOFOLLOW', 0))
            opened = os.fstat(fd)
            after = os.lstat(path)
            if (not stat.S_ISREG(before.st_mode) or not stat.S_ISREG(opened.st_mode)
                    or not stat.S_ISREG(after.st_mode) or opened.st_uid != os.geteuid()
                    or stat.S_IMODE(opened.st_mode) != 0o600
                    or (before.st_dev, before.st_ino) != (opened.st_dev, opened.st_ino)
                    or (after.st_dev, after.st_ino) != (opened.st_dev, opened.st_ino)):
                raise ValueError
            with os.fdopen(fd, 'rb') as stream:
                fd = -1
                data = stream.read(4097)
            if len(data) > 4096:
                raise ValueError
            token = data.decode('utf-8').rstrip('\r\n')
        except (OSError, ValueError, UnicodeError):
            raise RuntimeError('Could not read a secure live bridge token file') from None
        finally:
            if fd >= 0:
                os.close(fd)
    if len(token) < 32:
        raise RuntimeError('Live bridge token must contain at least 32 characters')
    try:
        token.encode('utf-8')
    except UnicodeEncodeError:
        raise RuntimeError('Live bridge token must be valid UTF-8') from None
    return token


def call(command, args, document_id=None, expected_revision=None):
    port_value = os.environ.get('CREABIM_MCP_PORT', '')
    try:
        port = int(port_value)
        if not 1 <= port <= 65535:
            raise ValueError
    except ValueError:
        raise RuntimeError('Set a valid CREABIM_MCP_PORT for the live bridge') from None
    token = _token()
    if command != 'inspect_document' and (not isinstance(document_id, str) or not document_id):
        raise ValueError('document_id is required; call live_inspect first')
    if command not in ('inspect_document', 'get_element') and (type(expected_revision) is not int or expected_revision < 0):
        raise ValueError('expected_revision is required for live edits; call live_inspect first')
    request = {'token': token, 'command': command, 'args': args}
    if document_id is not None:
        request['document_id'] = document_id
    if expected_revision is not None:
        request['expected_revision'] = expected_revision
    payload = (json.dumps(request, ensure_ascii=False, allow_nan=False, separators=(',', ':')) + '\n').encode('utf-8')
    if len(payload) > MAX_LINE:
        raise ValueError('Live bridge request exceeds 1 MiB')
    try:
        with socket.create_connection(('127.0.0.1', port), timeout=TIMEOUT) as conn:
            conn.sendall(payload)
            with conn.makefile('rb') as stream:
                raw = stream.readline(MAX_LINE + 1)
    except OSError as e:
        raise RuntimeError('Live bridge connection failed or timed out; inspect the active document before retrying') from e
    if not raw.endswith(b'\n') or len(raw) > MAX_LINE:
        raise RuntimeError('Live bridge returned an incomplete or oversized response; inspect the active document before retrying')
    try:
        response = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError):
        raise RuntimeError('Live bridge returned invalid JSON; inspect the active document before retrying') from None
    if not isinstance(response, dict) or type(response.get('ok')) is not bool:
        raise RuntimeError('Live bridge returned an invalid response; inspect the active document before retrying')
    if not response['ok']:
        message = str(response.get('error', 'Live bridge rejected the command')).replace(token, '[redacted]')
        raise RuntimeError(message)
    result = response.get('result')
    return result if isinstance(result, dict) else {'result': result}
