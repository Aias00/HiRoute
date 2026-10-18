"""Explicit HTTP admission and safe validation errors through real Local Control/save/restart."""
import hashlib
import json
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace
from urllib.parse import urlsplit

from model_connections_product import (
    control, desktop_grant, recheck_saved_native_source, save_native_source,
)
from publication_product import Product, encoded


def check_invalid(product, label, **changes):
    draft = {
        'candidate_ref': None, 'lineage_ref': 'lineage/http/' + label,
        'display_name': 'HTTP validation fixture', 'existing_source_id': None,
        'expected_source_revision': None, 'edit_revision': 1,
        'check_id': 'check/http/' + label, 'base_url': 'http://gateway.invalid/v1',
        'base_kind': 'api_root', 'request_path_override': None,
        'inventory_path_override': None, 'protocol': 'responses',
        'protocol_profile_id': 'profile/custom/responses', 'protocol_profile_revision': 1,
        'authentication': {'kind': 'none'}, 'configuration_revision': 1,
        'inference_model_id': None, 'models': [], 'additional_endpoints': [],
        **changes,
    }
    request = {'draft': draft}
    revisions = control(product, 'GetClientServiceStatus', {})['data']['revisions']
    grant = desktop_grant(product, 'CheckNativeModelConnection',
                          'sha256:' + hashlib.sha256(encoded(request)).hexdigest(),
                          revisions, 'http-' + label)
    envelope = control(product, 'CheckNativeModelConnection', request, grant,
                       expected_error='INVALID_ARGUMENTS')
    assert envelope.get('data') is None, envelope
    assert 'must-not-leak' not in json.dumps(envelope), envelope
    return envelope['error']['message_key']


def main():
    repository = Path(sys.argv[1]).resolve()
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repository, text=True).strip()
    if len(sys.argv) > 2:
        assert revision == sys.argv[2], (revision, sys.argv[2])
    product = Product(repository)
    saved_sources = []
    # No directory or inference operation is requested: these synthetic addresses
    # must be admitted and saved without DNS, network access or provider credentials.
    endpoints = [
        'http://192.0.2.10:8123/v1', 'http://192.168.1.10:8123/v1',
        'http://gateway.invalid:8123/v1', 'http://localhost:8123/v1',
        'http://127.0.0.1:8123/v1', 'http://[::1]:8123/v1',
        'http://[2001:db8::10]:8123/v1', 'https://gateway.invalid/v1',
    ]
    try:
        product.start()
        for index, endpoint in enumerate(endpoints):
            saved = save_native_source(product, SimpleNamespace(base_url=endpoint),
                                       token='synthetic-http-key-' + str(index) if index % 2 else None,
                                       variant='http-' + str(index), cli_input=True,
                                       source_display_name='HTTP fixture ' + str(index),
                                       inventory_path=None)
            saved_sources.append((saved, endpoint))
        for label, changes, expected in [
            ('url', {'base_url': 'http://user:must-not-leak@gateway.invalid/v1'},
             'model_connections.invalid_url'),
            ('scheme', {'base_url': 'ftp://gateway.invalid/v1'}, 'model_connections.invalid_url'),
            ('path', {'request_path_override': '/v1/../must-not-leak'}, 'model_connections.unsafe_path'),
            ('directory', {'inventory_path_override': '/v1/models?key=must-not-leak'},
             'model_connections.unsafe_path'),
            ('header', {'authentication': {'kind': 'api_key_header', 'header': 'Host'}},
             'model_connections.unsafe_authentication_header'),
            ('extra-url', {'additional_endpoints': [{
                'base_url': 'http://user:must-not-leak@gateway.invalid/v1', 'base_kind': 'api_root',
                'request_path_override': None, 'inventory_path_override': None,
                'protocol': 'messages', 'protocol_profile_id': 'profile/custom/messages',
                'protocol_profile_revision': 1, 'authentication': {'kind': 'none'},
             }]}, 'model_connections.invalid_url'),
        ]:
            assert check_invalid(product, label, **changes) == expected, label
        product.stop()
        product.start()
        sources = control(product, 'ListCompute', {})['data']['sources']
        for index, (saved, endpoint) in enumerate(saved_sources):
            source = next(item for item in sources if item['source_id'] == saved['source_id'])
            url = urlsplit(endpoint)
            assert source['target']['scheme'] == url.scheme, source
            assert source['target']['authority'] == url.hostname, source
            assert source['target']['port'] == (url.port or 443), source
            assert source.get('inventory_path') is None, source
            checked = recheck_saved_native_source(product, saved, 2, 'http-' + str(index),
                                                  directory='not_run')
            assert checked['target'] == source['target'], (checked, source)
            assert checked['inference'] == 'not_run', checked
        print(json.dumps({'scenario': 'custom-http-save-recheck-and-errors', 'state': 'green',
                          'revision': revision, 'saved_and_rechecked': len(saved_sources),
                          'validation_errors': 6}), flush=True)
    finally:
        product.close()


if __name__ == '__main__':
    main()
