"""Real managed Native source -> branch publication -> Gateway, including live permission edits."""
import http.server
import json
import sys
import threading
import time

from model_connections_product import (
    MODEL, NATIVE_TOKEN, NativeUpstream, assert_live_credential_switches,
    assert_session_record, edit_saved_credentials, gateway_request,
    publish_plan_and_agent, save_native_source,
)
from publication_product import Product, encoded


class Decision(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        selected = body['decision']['options'][0]
        result = {'kind': 'categorical', 'choice': selected['id']}
        if selected.get('refinement') is not None:
            result['refinement'] = {'kind': 'ordinal', 'probabilities': {'simple': 1, 'complex': 0}}
        raw = encoded({'decision': result})
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


def scenario(repository):
    product, upstream = Product(repository), NativeUpstream()
    decision = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Decision)
    worker = threading.Thread(target=decision.serve_forever, daemon=True)
    worker.start()
    product.install_codex_fixture(MODEL, 'medium', upstream.base_url, NATIVE_TOKEN)
    started = int(time.time() * 1000)
    try:
        product.start()
        saved = save_native_source(product, upstream, NATIVE_TOKEN, cli_input=True)
        replacement = edit_saved_credentials(product, upstream, saved)
        product.install_codex_fixture(MODEL, 'medium', upstream.base_url, replacement)
        service = {'id': 'native-branch-decision', 'revision': 1, 'name': 'Branch decision',
                   'connection': {'kind': 'custom', 'timeout_ms': 3000, 'auth_header': None,
                                  'endpoint': f'http://127.0.0.1:{decision.server_port}/decisions'}}
        spec = {'schema_version': {'major': 1, 'minor': 0}, 'command_id': 'decision.services.apply',
                'resource_id': service['id'], 'desired_state': {
                    'id': service['id'], 'expected_revision': 0, 'service': service}}
        preview = product.public_cli('decision services apply', {
            'schema_version': {'major': 1, 'minor': 0}, 'spec': spec})[1]['data']
        applied = product.public_cli('decision services apply', {
            'schema_version': {'major': 1, 'minor': 0}, 'spec': preview['normalized_spec'],
            'accept_digest': preview['change_digest'], 'expected_revisions': preview['expected_revisions'],
            'idempotency_key': 'native-branch-service'})[1]['data']
        assert applied['state'] == 'succeeded', applied
        publish_plan_and_agent(product, saved['binding_id'], branch_service=service)
        gateway_request(product)
        assert_session_record(product, started)
        assert_live_credential_switches(product, upstream, saved)
        assert_session_record(product, started)
        print(json.dumps({'state': 'green', 'scenario': 'native-branch-current-credentials',
                          'upstream_attempts': 3, 'disabled_requests_without_upstream': 6,
                          'restart_preserved_branch_and_permission': True}))
    finally:
        product.close()
        upstream.close()
        decision.shutdown()
        decision.server_close()
        worker.join(timeout=5)


if __name__ == '__main__':
    scenario(sys.argv[1])
