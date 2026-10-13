"""Current managed Key order through production save, exact leases and Gateway relay."""
import hashlib
import http.client
import json
import sys
from model_connections_product import (
    MODEL, NATIVE_TOKEN, NativeUpstream, control, gateway_request,
    publish_plan_and_agent, save_native_source,
)
from publication_product import Product, encoded
from plan_content_process import read_content


def save_source(product, saved, edits, label, disabled=False):
    snapshot = control(product, 'ListCompute', {})['data']
    source = next(row for row in snapshot['sources'] if row['source_id'] == saved['source_id'])
    change = {
        'schema': 'hiroute.compute-management-change/v2',
        'subject': {'kind': 'saved_source', 'source_id': saved['source_id']},
        'expected_revisions': snapshot['revisions'],
        'selected_model_refs': [model['model_ref'] for model in source['models']],
        'intent': 'save_disabled' if disabled else 'save_ready', 'key_edits': edits,
    }
    preview = control(product, 'PreviewComputeSave', {'change': change})['data']
    applied = control(product, 'ApplyComputeSave', {
        'spec': preview['spec'], 'accept_digest': preview['accept_digest'],
        'expected_revisions': preview['expected_revisions'], 'idempotency_key': 'pool-' + label,
    })
    result = control(product, 'GetComputeSaveResult', {'operation': applied['operation']})['data']
    assert result['disposition'] == 'saved', result


def run(repository):
    product = Product(repository)
    upstream = NativeUpstream()
    product.install_codex_fixture(MODEL, 'medium', upstream.base_url, NATIVE_TOKEN)
    try:
        product.start()
        saved = save_native_source(product, upstream, NATIVE_TOKEN)
        publish_plan_and_agent(product, saved['binding_id'])
        frozen_content = read_content(product)

        def source_snapshot():
            snapshot = control(product, 'ListCompute', {})['data']
            source = next(row for row in snapshot['sources'] if row['source_id'] == saved['source_id'])
            return snapshot, source

        def save(edits, label, disabled=False):
            save_source(product, saved, edits, label, disabled)
            assert read_content(product) == frozen_content, 'source save rewrote the frozen Plan'
            return source_snapshot()[1]

        def register(label, token):
            candidate = {'candidate_ref': 'candidate/pool/' + label, 'candidate_revision': 1}
            product.secrets.add(token)
            product.register_protected_frame({
                'schema': 'hiroute.protected-input/v1',
                'registration_id': hashlib.sha256(label.encode()).hexdigest(),
                **candidate, 'secret': token,
            })
            return candidate

        first = source_snapshot()[1]['keys'][0]['key_id']
        token_b = 'fixture-rejected-key-b-903b'
        upstream.rejected_token = token_b
        candidate_b = register('add-b', token_b)
        second = 'credential/managed-' + hashlib.sha256(encoded([
            'hiroute.compute-management-key/v2', saved['source_id'], candidate_b['candidate_ref'],
        ])).hexdigest()[:24]
        after = save([
            {'action': 'add', 'input_candidate': candidate_b},
            {'action': 'set_order', 'key_ids': [second, first]},
        ], 'add-b-before-a')
        assert [key['key_id'] for key in after['keys']] == [second, first], after
        def request_keys(expected):
            with upstream.lock:
                before = len(upstream.requests)
            gateway_request(product, allowed=bool(expected))
            with upstream.lock:
                observed = upstream.requests[before:]
            assert [row['authorization'] for row in observed] == [
                'Bearer ' + token for token in expected
            ], observed

        # Run before querying the new masks API, so the old runtime fails on the
        # actual missing B401 -> A200 relay rather than an unknown query operation.
        request_keys([token_b, NATIVE_TOKEN])
        masks = control(product, 'GetComputeKeyMasksV1', {
            'source_id': saved['source_id'], 'expected_source_revision': after['revision'],
        })['data']
        assert [key['key_id'] for key in masks['keys']] == [second, first], masks
        assert masks['keys'][0]['masked_key'] == 'fixt••••••••903b', masks
        assert all(token not in json.dumps(masks) for token in (NATIVE_TOKEN, token_b))
        control(product, 'GetComputeKeyMasksV1', {
            'source_id': saved['source_id'], 'expected_source_revision': after['revision'] - 1,
        }, expected_error='REVISION_CONFLICT')

        after = save([{'action': 'set_order', 'key_ids': [first, second]}], 'a-before-b')
        request_keys([NATIVE_TOKEN])
        # A previous 401 retains its existing cooldown. Rotate B to a healthy
        # generation before verifying the restored B-first pool.
        replacement = 'fixture-replacement-key-b-47af'
        after = save([
            {'action': 'replace', 'key_id': second, 'expected_generation': 1,
             'input_candidate': register('replace-b', replacement)},
            {'action': 'set_order', 'key_ids': [second, first]},
        ], 'b-before-a-again')
        request_keys([replacement])
        after = save([], 'disable-source', disabled=True)
        request_keys([])
        product.stop()
        product.start()
        request_keys([])
        after = save([], 'restore-source')
        request_keys([replacement])
        after = save([{'action': 'remove', 'key_id': second, 'expected_generation': 2}], 'remove-b')
        request_keys([NATIVE_TOKEN])
        assert [key['key_id'] for key in after['keys']] == [first], after
        assert read_content(product) == frozen_content
        print(json.dumps({'scenario': 'current-key-pool-with-frozen-plan', 'state': 'green',
                          'one_save_order': True, 'relay_401_then_200': True,
                          'plan_republished': False, 'restored_order_after_restart': True,
                          'replacement_and_removal': True, 'saved_key_masks': True}), flush=True)
    finally:
        product.close()
        upstream.close()


def run_filtered_history(repository):
    """An unavailable economy source cannot authorize cleanup on the prior primary."""
    product = Product(repository)
    economy, primary = NativeUpstream(), NativeUpstream()
    product.install_codex_fixture(MODEL, 'medium', primary.base_url, NATIVE_TOKEN)

    def message(text):
        return {'type': 'message', 'role': 'user',
                'content': [{'type': 'input_text', 'text': text}]}

    def request_history(items):
        client = http.client.HTTPConnection('127.0.0.1', product.port, timeout=30)
        try:
            client.request('POST', '/v1/responses', body=encoded({
                'model': product.model_alias, 'input': items, 'stream': False,
            }), headers={
                'X-HiRoute-Token': product.bearer(product.agent_connection),
                'Content-Type': 'application/json', 'session-id': 'filtered-pool-history',
            })
            response = client.getresponse()
            product.outputs.append(response.read())
            return response.status
        finally:
            client.close()

    try:
        product.start()
        a = save_native_source(product, economy, 'fixture-history-key-a', variant='history-a')
        b = save_native_source(product, primary, 'fixture-history-key-b', variant='history-b')
        publish_plan_and_agent(product, b['binding_id'], native_model_mode='hiroute_only',
                               economy_binding=a['binding_id'])
        frozen_content = read_content(product)
        assert request_history([message('complex-route initial work')]) == 200
        assert not [row for row in economy.requests if row['method'] == 'POST']
        assert len([row for row in primary.requests if row['method'] == 'POST']) == 1
        save_source(product, a, [], 'disable-history-a', disabled=True)
        assert read_content(product) == frozen_content
        primary.reject_reasoning_history = True
        history = [{'type': 'reasoning', 'summary': [],
                    'encrypted_content': 'keep-for-current-b'}, message('rename after compaction')]
        assert request_history(history) >= 400, 'same candidate unexpectedly cleaned history'
        assert not [row for row in economy.requests if row['method'] == 'POST']
        posts = [row for row in primary.requests if row['method'] == 'POST']
        assert len(posts) == 2, 'same candidate received an unauthorized cleanup retry'
        assert posts[-1]['body']['input'][0]['encrypted_content'] == 'keep-for-current-b'
        assert read_content(product) == frozen_content
        print(json.dumps({'scenario': 'filtered-key-pool-preserves-current-candidate-history',
                          'state': 'green', 'disabled_source_posts': 0,
                          'current_candidate_posts': 1, 'unauthorized_cleanup_retry': False,
                          'history_preserved': True, 'plan_republished': False}), flush=True)
    finally:
        product.close()
        economy.close()
        primary.close()


if __name__ == '__main__':
    if len(sys.argv) > 2 and sys.argv[2] == 'filtered-history':
        run_filtered_history(sys.argv[1])
    else:
        run(sys.argv[1])
