#!/usr/bin/env python3
'''Simple mock Chain RPC server for dchat validator bootstrap'''
import json
import http.server
import socketserver
import uuid
import time

class ChainRPCHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        print(f"[{time.strftime('%Y-%m-%d %H:%M:%S')}] {format % args}")
    
    def do_POST(self):
        content_length = int(self.headers.get('Content-Length', 0))
        body = self.rfile.read(content_length).decode('utf-8')
        
        try:
            request = json.loads(body)
            method = request.get('method', '')
            params = request.get('params', {})
            request_id = request.get('id', 1)
            
            print(f"RPC Call: {method}")
            print(f"  Params: {json.dumps(params, indent=2)}")
            
            # Handle staking methods
            if method == 'currency.stake_validator':
                response = {
                    'jsonrpc': '2.0',
                    'id': request_id,
                    'result': {
                        'tx_id': str(uuid.uuid4()),
                        'block_height': int(time.time()) % 1000000,
                        'status': 'confirmed'
                    }
                }
            elif method == 'currency.stake_relay':
                response = {
                    'jsonrpc': '2.0',
                    'id': request_id,
                    'result': {
                        'tx_id': str(uuid.uuid4()),
                        'block_height': int(time.time()) % 1000000,
                        'status': 'confirmed'
                    }
                }
            elif method == 'chain.height':
                response = {
                    'jsonrpc': '2.0',
                    'id': request_id,
                    'result': {'height': int(time.time()) % 1000000}
                }
            elif method == 'chain.status':
                response = {
                    'jsonrpc': '2.0',
                    'id': request_id,
                    'result': {'syncing': False, 'chain_id': 'dchat-mainnet-1'}
                }
            else:
                # Generic success for unknown methods
                response = {
                    'jsonrpc': '2.0',
                    'id': request_id,
                    'result': {'status': 'ok', 'method': method}
                }
            
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps(response).encode('utf-8'))
            print(f"  Response: {json.dumps(response)}")
            
        except Exception as e:
            print(f"Error: {e}")
            self.send_response(500)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            error_response = {
                'jsonrpc': '2.0',
                'id': 1,
                'error': {'code': -32603, 'message': str(e)}
            }
            self.wfile.write(json.dumps(error_response).encode('utf-8'))

if __name__ == '__main__':
    PORT = 26657
    print(f'Starting mock Chain RPC server on port {PORT}...')
    with socketserver.TCPServer(('0.0.0.0', PORT), ChainRPCHandler) as httpd:
        print(f'Mock RPC ready at http://localhost:{PORT}')
        httpd.serve_forever()
