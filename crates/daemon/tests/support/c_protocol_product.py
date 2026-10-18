"""Opt-in C through real CLI, retained versions, publication and explicit diagnostics.

The only upstream is the caller's counted local deterministic HTTP fixture.
"""
import copy
import hashlib
import http.client
import json
import time
from publication_process import plan_change
from publication_product import encoded

C = 'hiroute.system-one-c/v1'
LEGACY = 'hiroute.system-one-legacy/v1'


def digest(value):
    return 'sha256:' + hashlib.sha256(encoded(value)).hexdigest()


def native_protocol(repository):
    stock = json.loads((repository / 'crates/domain/src/routing/fixtures/system_one_c_v1.json').read_text())
    assert digest(stock) == 'sha256:4dbab2f53954eb2176b6aa8d31b97808c4fee15ebb2fae337012eba348aef3c7'
    wrapper, suffix = stock['q1']['instructions'].split(' Distinguish those meanings', 1)
    return {'schema': C, 'prior_context_max_turns': 8}, {
            'instructions': 'Distinguish those meanings' + suffix, 'criteria': stock['q1']['criteria']}



def frozen_from(preview):
    def find(value):
        if isinstance(value, dict):
            if value.get('context_layout') == 'hiroute.partitioned-history/v1':
                return value
            for child in value.values():
                found = find(child)
                if found is not None:
                    return found
        elif isinstance(value, list):
            for child in value:
                found = find(child)
                if found is not None:
                    return found
        return None
    value = find(preview['plan_version']['compiled'])
    assert value is not None, 'compiler lost frozen C semantics'
    assert value['digest'] == digest([value['protocol'], value['context_layout'], value['decision'], value['scopes']])
    return value


def exercise(product, cli, service, endpoint, selection):
    protocol, stock = native_protocol(product.repo)
    smart = copy.deepcopy(product.editor['smart'])
    smart.update(economy=[selection], primary=[selection], classifier={'kind': 'decision_service', 'service': service})
    smart['judgment']['degree']['simple_threshold_millis'] = 817
    smart['judgment']['competence']['floor_millis'] = 317
    original = copy.deepcopy(smart['judgment'])
    calls = len(endpoint.requests)

    def change_for(editor):
        return plan_change(product, 'update', **editor)

    def publish(editor, key):
        change = change_for(editor)
        preview = product.preview('routing preview', {'change': change})
        assert 'assessment_transition' not in preview and 'assessment_choices' not in change
        product.apply('routing apply', 'ApplyAgentPlanChange', preview, {'change': change}, key)
        product.stop()
        product.start()
        assert len(endpoint.requests) == calls, 'save/publication/restart implicitly probed a provider'
        saved = cli('routing show ' + product.plan_id)['data']
        assert 'assessment_transition' not in saved
        return preview, saved

    def judgment(saved):
        return saved['desired']['strategy']['judgment']

    editor = copy.deepcopy(product.editor)
    editor.update(mode='smart_saving', smart=smart)
    _, legacy = publish(editor, 'applicability-off-default')
    legacy_identity = copy.deepcopy(legacy['head']['reference'])
    smart['native_protocol'] = protocol
    preview, saved = publish(editor, 'applicability-on-default')
    frozen = frozen_from(preview)
    assert 'fixed' not in protocol and 'digest' not in protocol
    assert frozen['protocol']['schema'] == C and 'fixed' in frozen['protocol']
    assert frozen['scopes']['smart_saving']['standard'] == 'default'
    assert judgment(saved)['degree'] == original['degree']
    assert judgment(saved)['competence']['floor_millis'] == 317
    assert judgment(saved)['competence']['instructions'] == stock['instructions']
    smart['judgment'] = copy.deepcopy(judgment(saved))
    smart.pop('native_protocol')
    _, saved = publish(editor, 'applicability-off-default-again')
    assert judgment(saved)['competence']['instructions'] == original['competence']['instructions']
    assert judgment(saved)['competence']['criteria'] == original['competence']['criteria']
    assert judgment(saved)['competence']['floor_millis'] == 317
    smart['judgment'] = copy.deepcopy(judgment(saved))
    smart['native_protocol'] = protocol
    _, saved = publish(editor, 'applicability-on-default-again')
    smart['judgment'] = copy.deepcopy(judgment(saved))
    smart['judgment']['competence']['standard'] = 'custom'
    smart['judgment']['competence']['instructions'] += '\n  Assess the requested deliverable.  '
    smart['judgment']['competence']['criteria'][1] += '  '
    custom = copy.deepcopy(smart['judgment'])
    for enabled in [True, False, True]:
        if enabled:
            smart['native_protocol'] = protocol
        else:
            smart.pop('native_protocol', None)
        preview, saved = publish(editor, 'applicability-custom-' + str(enabled) + '-' + str(len(endpoint.requests)) + '-' + str(time.monotonic_ns()))
        assert judgment(saved) == custom, 'toggle rewrote custom text or thresholds'
    frozen = frozen_from(preview)
    assert frozen['scopes']['smart_saving']['standard'] == 'custom'
    bad = copy.deepcopy(editor)
    bad['smart']['judgment']['competence']['instructions'] += ' Use state.visible_conversation.'
    rejected = cli('routing preview', {'change': change_for(bad)}, ok=False)
    assert rejected['status'] != 'succeeded' and rejected['data']['field'] == 'smart_saving.competence.instructions', rejected

    legacy_test = cli('decision services test', {'schema': 'hiroute.classifier-decision-test/v1',
        'classifier': smart['classifier']})['data']
    calls += 1
    assert legacy_test['outcome'] == 'passed' and 'state' not in legacy_test
    request = {'schema': 'hiroute.historical-applicability-test/v1', 'service_id': service['id'],
               'service_revision': service['revision'], 'protocol': frozen, 'shape': 'standard'}
    endpoint.c_vectors = False
    negative = cli('decision services test', request)['data']
    calls += 1
    assert negative['state'] == 'unsupported', negative
    endpoint.c_vectors = True
    positive = cli('decision services test', request)['data']
    calls += 1
    assert positive['state'] == 'verified' and positive['question_count'] == 3, positive
    assert positive['service_revision'] == service['revision'] and positive['protocol_digest'] == frozen['digest']
    assert positive['endpoint_digest'] == digest(service['connection']['endpoint'])
    body = endpoint.requests[-1][1]
    assert set(body['state']) == {'latest_user', 'prior_context', 'assessment_stage', 'history_partial'}
    assert body['state']['assessment_stage'] and len(body['questions']) == 3
    stale = copy.deepcopy(request); stale['service_revision'] = 999
    assert cli('decision services test', stale, ok=False)['status'] != 'succeeded'
    assert len(endpoint.requests) == calls, 'missing saved revision dispatched a request'

    routing_calls = 0
    if product.cpa_args:
        routing_calls = execute_and_inspect(product, endpoint, saved)
        calls += routing_calls

    branches = [{'id': f'category-{i}', 'name': f'Category {i}', 'condition': f'Synthetic work {i}',
                 'candidates': [selection], 'primary_candidates': [selection], 'judgment': None} for i in range(16)]
    default = copy.deepcopy(original)
    default['competence']['standard'] = 'default'
    routing = {'classifier': smart['classifier'], 'branches': branches, 'default_branch_id': branches[0]['id'],
               'judgment': default, 'native_protocol': protocol, 'reselect_on_user_message': False}
    maximum_change = plan_change(product, 'update', mode='custom_branches', branch_routing=routing)
    maximum = frozen_from(product.preview('routing preview', {'change': maximum_change}))
    assert all(scope['standard'] == 'default' for scope in maximum['scopes'].values())
    request.update(protocol=maximum, shape='max_branches')
    result = cli('decision services test', request)['data']
    calls += 1
    assert result['state'] == 'verified' and result['question_count'] == 19, result
    assert len(endpoint.requests[-1][1]['questions']) == 19

    smart.pop('native_protocol')
    _, saved = publish(editor, 'applicability-off-custom-final')
    assert judgment(saved) == custom
    if product.cpa_args:
        calls += inspect_mixed_after_rollback(product, endpoint)
        routing_calls += 1
    # Reset uses only the current default, with no retained-revision dependency.
    smart['judgment']['competence']['standard'] = 'default'
    smart['judgment']['competence']['instructions'] = original['competence']['instructions']
    smart['judgment']['competence']['criteria'] = original['competence']['criteria']
    _, reset = publish(editor, 'applicability-reset-default')
    assert judgment(reset)['competence']['instructions'] == original['competence']['instructions']
    assert judgment(reset)['degree'] == original['degree']
    endpoint.c_vectors = False
    print(json.dumps({'scenario': 'default-custom-toggle-restart-connectivity-and-purpose-named-applicability-probes',
                      'state': 'green', 'explicit_calls': 4, 'routing_calls': routing_calls, 'implicit_calls': 0}), flush=True)


def execute_and_inspect(product, endpoint, saved):
    """Actual Gateway -> event ingestion -> existing retained query, with local upstreams."""
    started = int(time.time() * 1000)
    product.c_observation_from_ms = started - 1
    before = len(endpoint.requests)
    endpoint.c_score = 0.2
    endpoint.c_applicability = 0.25
    messages = [{'role': 'user', 'content': 'Complete the first deterministic C stage.'}]
    for index in range(2):
        client = http.client.HTTPConnection('127.0.0.1', product.port, timeout=30)
        try:
            client.request('POST', '/v1/responses', encoded({'model': product.model_alias, 'input': messages, 'stream': True}),
                {'Content-Type': 'application/json', 'X-HiRoute-Token': product.bearer(product.agent_connection),
                 'session-id': 'c-protocol-product-retained'})
            response = client.getresponse()
            body = response.read()
            assert response.status == 200, (response.status, body[:500])
            completed = [json.loads(line[6:]) for line in body.splitlines() if line.startswith(b'data: {')]
            answer = next(event['response'] for event in completed if event.get('type') == 'response.completed')
            text = ''.join(part['text'] for item in answer['output'] if item['type'] == 'message'
                           for part in item['content'] if part['type'] == 'output_text')
            assert text
            if index == 0:
                messages.extend([{'role': 'assistant', 'content': text},
                                 {'role': 'user', 'content': 'Continue with the next deterministic C step.'}])
        finally:
            client.close()
    assert len(endpoint.requests) == before + 2
    assert endpoint.requests[-1][1]['state']['assessment_stage'], 'actual completed stage was not attributed'
    assert len(endpoint.requests[-1][1]['questions']) == 3, 'required applicability question was omitted'

    query = {'plan_id': product.plan_id, 'plan_revision': saved['head']['reference']['content_revision'],
             'include_routing_observations': True, 'from_ms': started - 1, 'to_ms': int(time.time() * 1000) + 60000, 'limit': 20}
    deadline = time.monotonic() + 30
    def read(options, schema='hiroute.observation.query/v2'):
        # Ingestion can advance generation after a read transaction commits. A
        # fresh first page may then report stale; never retry other errors or an
        # old cursor, and keep the original acceptance deadline.
        assert not options.get('cursor')
        while True:
            result = product.control('GetPlanQualitySamples', {'schema': schema,
                'intent': {'view': 'plan_quality', 'query': options}}, success=False)
            error = result.get('error')
            if error is None:
                return result['data']
            assert error['code'] == 'CHANGE_PREVIEW_STALE', result
            assert time.monotonic() < deadline, 'Observation generation did not settle before the existing deadline'
            time.sleep(.05)
    while True:
        page = read(query)
        summary = page['routing_observations']
        if summary['retained_decisions'] >= 2:
            break
        assert time.monotonic() < deadline, 'C decisions did not reach retained product observations'
        time.sleep(.05)
    assert summary['coverage'] == 'complete', summary
    assert len(summary['cohorts']) == 1, summary
    counts = summary['cohorts'][0]['counts']
    assert counts['c_decisions'] == 2 and counts['requested'] == 1 and counts['unavailable'] == 0, counts
    assert counts['gate_opportunities'] == 1 and counts['suppressed'] == 1, counts
    assert counts['suppressed_per_opportunity'] == 1.0 and counts['suppressed_per_c_decision'] == 0.5, counts
    filtered = read({**query, 'model_configuration_id': 'model/no-matching-execution'})
    assert filtered['samples'] == [] and filtered['routing_observations'] == summary
    strict = read({**query, 'include_routing_observations': False})
    assert 'schema' not in strict and 'routing_observations' not in strict
    assert strict['samples'], 'strict nested-shape regression selected no stage samples'
    assert all('routing_observation' not in (sample.get('selection') or {}) and 'protocol' not in (sample.get('selection') or {}) for sample in strict['samples'])
    endpoint.c_score, endpoint.c_applicability = 1.0, 0.75
    print(json.dumps({'scenario': 'c-real-gateway-to-retained-product-observations', 'state': 'green',
                      'routing_calls': 2, 'requested': 1, 'protection_opportunities': 1, 'suppressed': 1}), flush=True)
    return 2


def inspect_mixed_after_rollback(product, endpoint):
    """A current legacy decision and C decisions share one complete retained Plan scope."""
    before = len(endpoint.requests)
    client = http.client.HTTPConnection('127.0.0.1', product.port, timeout=30)
    try:
        client.request('POST', '/v1/responses', encoded({'model': product.model_alias,
            'input': 'Execute the deterministic step after explicit rollback.', 'stream': True}),
            {'Content-Type': 'application/json', 'X-HiRoute-Token': product.bearer(product.agent_connection),
             'session-id': 'c-protocol-product-retained'})
        response = client.getresponse(); body = response.read()
        assert response.status == 200, (response.status, body[:500])
    finally:
        client.close()
    assert len(endpoint.requests) == before + 1
    query = {'plan_id': product.plan_id, 'include_routing_observations': True, 'from_ms': product.c_observation_from_ms,
             'to_ms': int(time.time() * 1000) + 60000, 'limit': 20}
    deadline = time.monotonic() + 30
    while True:
        result = product.control('GetPlanQualitySamples', {'schema': 'hiroute.observation.query/v2',
            'intent': {'view': 'plan_quality', 'query': query}}, success=False)
        if result.get('error'):
            assert result['error']['code'] == 'CHANGE_PREVIEW_STALE', result
        else:
            summary = result['data']['routing_observations']
            if summary['retained_decisions'] >= 3:
                break
        assert time.monotonic() < deadline, 'Rolled-back native decision did not reach retained observations'
        time.sleep(.05)
    assert summary['coverage'] == 'complete' and summary['known_non_c_decisions'] == 1 and summary['legacy_or_unknown_decisions'] == 0, summary
    counts = summary['cohorts'][0]['counts']
    assert counts['c_decisions'] == 2 and counts['suppressed_per_opportunity'] == 1.0 and counts['suppressed_per_c_decision'] == 0.5, counts
    print(json.dumps({'scenario':'current-c-and-legacy-all-revisions-product-query','state':'green',
                      'known_non_c_decisions':1,'c_decisions':2,'coverage':'complete'}), flush=True)
    return 1
