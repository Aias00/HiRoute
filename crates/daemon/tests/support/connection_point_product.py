"""Released CLI -> real daemon -> exact multi-model save, using passive native fixtures."""
import json
from pathlib import Path
import sys
from publication_product import Product, encoded


def write(path, value):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_bytes(value if isinstance(value, bytes) else encoded(value))
    path.chmod(0o600)


def data(product, command, payload=None):
    return product.public_cli(command, payload)[1]['data']


def prepare(product, point, name):
    return data(product, 'compute connection prepare', {
        'discovery': point['discovery'], 'prepare_id': name,
    })


def preview(product, candidate, selected):
    snapshot = data(product, 'compute list')
    return data(product, 'compute connection preview', {'change': {
        'schema': 'hiroute.compute-management-change/v2',
        'subject': {'kind': 'candidate', 'candidate': candidate['candidate']},
        'expected_revisions': snapshot['revisions'], 'selected_model_refs': selected,
        'intent': 'save_ready', 'key_edits': [],
    }})


def apply(product, draft, key, success=True):
    return product.public_cli('compute connection apply', {
        'spec': draft['spec'], 'accept_digest': draft['accept_digest'],
        'expected_revisions': draft['expected_revisions'], 'idempotency_key': key,
    }, success=success)


def run(repository):
    product = Product(repository)
    try:
        hermes = product.root / 'selected-hermes-profile'
        hermes.mkdir(mode=0o700)
        hermes_config = {'model': {'provider': 'team-native', 'default': 'first'}, 'providers': {
            'team-native': {'api': 'https://point.example.invalid/v1', 'transport': 'openai_chat',
                            'key_env': 'POINT_PROFILE_KEY', 'models': {'first': {}, 'second': {}}}}}
        write(hermes / 'config.yaml', hermes_config)
        write(hermes / '.env', b'POINT_PROFILE_KEY=point-profile-fixture-secret\nALTERNATIVE_KEY=point-profile-fixture-secret\n')
        product.secrets.add('point-profile-fixture-secret')
        pi = product.root / 'home/.pi/agent/models.json'
        write(pi, {'providers': {'team-native': {
            'baseUrl': 'https://point.example.invalid/v1', 'api': 'openai-responses', 'apiKey': 'pi-point-fixture-secret',
            'models': [{'id': 'first'}, {'id': 'second'}, {'id': 'unavailable', 'contextWindow': 0}],
        }}})
        product.secrets.add('pi-point-fixture-secret')
        before = {path: path.read_bytes() for path in [hermes / 'config.yaml', hermes / '.env', pi]}
        product.start()
        request = {'schema': 'hiroute.compute-scan-request/v2',
                   'native_target': {'kind': 'hermes', 'config_root': str(hermes)}}
        scan = data(product, 'compute scan', request)
        assert scan['schema'] == 'hiroute.compute-scan-result/v2'
        hermes_point = next(p for p in scan['connections'] if p['agent_id'] == 'agent_hermes_default')
        pi_point = next(p for p in scan['connections'] if p['agent_id'] == 'agent_pi_default')
        assert hermes_point['default_model_id'] == 'first'
        assert hermes_point['declared_model_ids'] == ['first', 'second']
        assert hermes_point['protocol'] == 'chat_completions'
        assert hermes_point['connection_ref'] != pi_point['connection_ref']
        assert len(pi_point['declared_model_ids']) == 3
        for point in [hermes_point, pi_point]:
            assert point['discovery'] and 'credential_binding' not in point and 'field_selector' not in point
        for version in ['hiroute.compute-scan-request/v3', 'unrecognized']:
            status, _ = product.public_cli('compute scan', {'schema': version}, success=False)
            assert status != 0
        legacy = data(product, 'compute scan')
        assert legacy['schema'] == 'hiroute.compute-scan-result/v1' and 'connections' not in legacy
        legacy_pi = next(p for p in legacy['items'] if p['agent_id'] == 'agent_pi_default' and p['observed_model_id'] == 'first')
        single = prepare(product, legacy_pi, 'prepare/point-product/legacy')
        assert [m['upstream_model_id'] for m in single['models']] == ['first']
        multiple = prepare(product, pi_point, 'prepare/point-product/multiple')
        assert {m['upstream_model_id'] for m in multiple['models']} == {'first', 'second', 'unavailable'}
        assert sum(m['selectable'] for m in multiple['models']) == 2
        candidate = prepare(product, hermes_point, 'prepare/point-product/hermes')
        assert {m['upstream_model_id'] for m in candidate['models']} == {'first', 'second'}
        assert all(m['membership'] == 'observed' for m in candidate['models'])
        assert all(path.read_bytes() == contents for path, contents in before.items()), 'Scan/prepare mutated a native source'
        assert not data(product, 'compute list')['sources'], 'Scan/prepare saved a source'
        selection = [m['model_ref'] for m in candidate['models'] if m['selectable']]
        draft = preview(product, candidate, selection[:1])
        # Changing only credential selection provenance still invalidates the snapshot.
        hermes_config['providers']['team-native']['key_env'] = 'ALTERNATIVE_KEY'
        write(hermes / 'config.yaml', hermes_config)
        status, _ = apply(product, draft, 'point-source-stale', success=False)
        assert status != 0 and not data(product, 'compute list')['sources']
        fresh = data(product, 'compute scan', request)
        point = next(p for p in fresh['connections'] if p['agent_id'] == 'agent_hermes_default')
        candidate = prepare(product, point, 'prepare/point-product/fresh')
        selection = [m['model_ref'] for m in candidate['models'] if m['selectable']]
        draft = preview(product, candidate, selection)
        admitted = apply(product, draft, 'point-exact-save')[1]['data']
        assert admitted['operation']['state'] == 'succeeded'
        snapshot = data(product, 'compute list')
        assert len(snapshot['sources']) == 1
        source = snapshot['sources'][0]
        assert {m['upstream_model_id'] for m in source['models']} == {'first', 'second'}
        assert source['target']['upstream_protocol'] == 'chat_completions'
        product.restart()
        snapshot = data(product, 'compute list')
        assert len(snapshot['sources']) == 1 and snapshot['sources'][0]['source_id'] == source['source_id']
        assert {m['upstream_model_id'] for m in snapshot['sources'][0]['models']} == {'first', 'second'}
        write(hermes / 'config.yaml', {**hermes_config, 'providers': {'team-native': {
            **hermes_config['providers']['team-native'], 'models': {'first': {}, 'second': {}, 'newly-added': {}}}}})
        data(product, 'compute scan', request)
        assert {m['upstream_model_id'] for m in data(product, 'compute list')['sources'][0]['models']} == {'first', 'second'}
        print(json.dumps({'scenario': 'released-cli-connection-points', 'state': 'green',
                          'cases': ['legacy-one-model', 'private-multi-model-prepare', 'hermes-default-chat',
                                    'unselectable-member', 'equal-secret-selector-drift', 'exact-save-restart', 'no-refresh-expansion']}), flush=True)
    finally:
        product.close()


if __name__ == '__main__':
    run(Path(sys.argv[1]))
