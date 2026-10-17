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
LEGACY_WRAPPER = 'Evaluate only the completed stage from state.visible_conversation[state.assessment_from] through the end. state.latest_user may supply explicit feedback, but is not yet executed.'


def digest(value):
    return 'sha256:' + hashlib.sha256(encoded(value)).hexdigest()


def native_protocol(repository):
    stock = json.loads((repository / 'crates/domain/src/routing/fixtures/system_one_c_v1.json').read_text())
    assert digest(stock) == 'sha256:4dbab2f53954eb2176b6aa8d31b97808c4fee15ebb2fae337012eba348aef3c7'
    wrapper, suffix = stock['q1']['instructions'].split(' Distinguish those meanings', 1)
    return {'schema': C, 'fixed': {'degree_prefix': 'Assess state.latest_user only, not historical competence.',
            'assessment_wrapper': wrapper, 'applicability': stock['q2']}, 'prior_context_max_turns': 8}, {
            'instructions': 'Distinguish those meanings' + suffix, 'criteria': stock['q1']['criteria']}


def review(protocol, judgment, method='reviewed_custom', source=None):
    question = {'type': 'score', 'instructions': (protocol['fixed']['assessment_wrapper'] if protocol else LEGACY_WRAPPER)
                + ' ' + judgment['competence']['instructions'], 'criteria': judgment['competence']['criteria']}
    result = {'protocol': C if protocol else LEGACY, 'assessment_digest': digest(question), 'method': method}
    if source is not None:
        result['source'] = source
    return result


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

    def publish(editor, key):
        change = plan_change(product, 'update', **editor)
        preview = product.preview('routing preview', {'change': change})
        product.apply('routing apply', 'ApplyAgentPlanChange', preview, {'change': change}, key)
        product.stop()
        product.start()
        assert len(endpoint.requests) == calls, 'save/publication/restart implicitly probed a provider'
        saved = cli('routing show ' + product.plan_id)['data']
        return preview, saved

    editor = copy.deepcopy(product.editor)
    editor.update(mode='smart_saving', smart=smart)
    _, legacy = publish(editor, 'c-legacy-source')
    legacy_ref = legacy['head']['reference']
    smart['native_protocol'] = protocol
    smart['judgment']['competence'].update(stock)
    invalid = plan_change(product, 'update', mode='smart_saving', smart=smart)
    rejected = cli('routing preview', {'change': invalid}, ok=False)
    assert rejected['status'] != 'succeeded', 'bare C toggle accepted without scope review'
    smart['assessment_review'] = {'smart_saving': review(protocol, smart['judgment'], 'stock')}
    editor['smart'] = smart
    preview, saved = publish(editor, 'c-adopt-reviewed-stock')
    frozen = frozen_from(preview)
    assert frozen['protocol'] == protocol
    assert saved['desired']['strategy']['judgment']['degree'] == original['degree']
    assert saved['desired']['strategy']['judgment']['competence']['floor_millis'] == 317
    # The existing options response remains strict unless this projection is requested.
    assert 'assessment_sources' not in cli('routing options', {})['data']
    sources = cli('routing options', {'assessment_review_plan': {'plan_id': product.plan_id,
                  'revision': saved['head']['head_revision']}})['data']['assessment_sources']
    source = next(s for s in sources if s['source']['scope'] == 'smart_saving')
    assert source['source']['plan'] == legacy_ref
    assert source['instructions'] == original['competence']['instructions']

    request = {'schema': 'hiroute.classifier-decision-test/v2', 'service_id': service['id'],
               'service_revision': service['revision'], 'protocol': frozen, 'shape': 'standard'}
    endpoint.c_vectors = False
    negative = cli('decision services test', request)['data']
    calls += 1
    assert negative['state'] == 'unsupported', negative
    endpoint.c_vectors = True
    positive = cli('decision services test', request)['data']
    calls += 1
    assert positive['state'] == 'verified' and positive['question_count'] == 3, positive
    assert positive['service_revision'] == service['revision']
    assert positive['protocol_digest'] == frozen['digest']
    assert positive['endpoint_digest'] == digest(service['connection']['endpoint'])
    body = endpoint.requests[-1][1]
    assert set(body['state']) == {'latest_user', 'prior_context', 'assessment_stage', 'history_partial'}
    assert body['state']['assessment_stage'] and len(body['questions']) == 3
    stale = copy.deepcopy(request)
    stale['service_revision'] = 999
    assert cli('decision services test', stale, ok=False)['status'] != 'succeeded'
    assert len(endpoint.requests) == calls, 'missing saved revision dispatched a request'

    routing_calls = 0
    if product.cpa_args:
        routing_calls = execute_and_inspect(product, endpoint, saved)
        calls += routing_calls

    # Full 16-branch shape is compiled by the product, then tested in one actual call.
    branches = [{'id': f'category-{i}', 'name': f'Category {i}', 'condition': f'Synthetic work {i}',
                 'candidates': [selection], 'primary_candidates': [selection], 'judgment': None} for i in range(16)]
    routing = {'classifier': smart['classifier'], 'branches': branches, 'default_branch_id': branches[0]['id'],
               'judgment': smart['judgment'], 'native_protocol': protocol,
               'assessment_review': {'default': review(protocol, smart['judgment'], 'stock')},
               'reselect_on_user_message': False}
    maximum_change = plan_change(product, 'update', mode='custom_branches', branch_routing=routing)
    maximum = frozen_from(product.preview('routing preview', {'change': maximum_change}))
    request.update(protocol=maximum, shape='max_branches')
    result = cli('decision services test', request)['data']
    calls += 1
    assert result['state'] == 'verified' and result['question_count'] == 19, result
    assert len(endpoint.requests[-1][1]['questions']) == 19

    # A C review cannot approve the legacy wrapper. Missing source is explicit/stale.
    rollback = copy.deepcopy(smart)
    rollback.pop('native_protocol')
    invalid = plan_change(product, 'update', mode='smart_saving', smart=rollback)
    assert cli('routing preview', {'change': invalid}, ok=False)['status'] != 'succeeded'
    rollback['judgment']['competence'].update(instructions=source['instructions'], criteria=source['criteria'])
    rollback['assessment_review'] = {'smart_saving': review(None, rollback['judgment'], 'historical', source['source'])}
    missing = copy.deepcopy(rollback)
    missing['assessment_review']['smart_saving']['source']['plan']['content_digest'] = 'sha256:' + 'f' * 64
    invalid = plan_change(product, 'update', mode='smart_saving', smart=missing)
    rejected = cli('routing preview', {'change': invalid}, ok=False)
    assert rejected['status'] != 'succeeded' and rejected['error']['code'] == 'CHANGE_PREVIEW_STALE', rejected
    editor['smart'] = rollback
    _, saved = publish(editor, 'c-restore-exact-retained-rubric')
    restored = saved['desired']['strategy']
    assert 'native_protocol' not in restored
    assert restored['judgment'] == original, 'rollback altered thresholds or degree text'
    assert restored['assessment_review']['smart_saving']['source'] == source['source']
    endpoint.c_vectors = False
    print(json.dumps({'scenario': 'c-reviewed-adoption-restart-explicit-3-and-19-question-diagnostics-exact-scope-rollback',
                      'state': 'green', 'explicit_calls': 3, 'routing_calls': routing_calls, 'implicit_calls': 0}), flush=True)


def execute_and_inspect(product, endpoint, saved):
    """Actual Gateway -> event ingestion -> existing retained query, with local upstreams."""
    started = int(time.time() * 1000)
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
             'from_ms': started - 1, 'to_ms': int(time.time() * 1000) + 60000, 'limit': 20,
             'include_routing_observations': True}
    def read(options):
        return product.control('GetPlanQualitySamples', {'schema': 'hiroute.observation.query/v2',
            'intent': {'view': 'plan_quality', 'query': options}})['data']
    deadline = time.monotonic() + 30
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
    assert 'routing_observations' not in strict
    assert all('routing_observation' not in (sample.get('selection') or {}) for sample in strict['samples'])
    endpoint.c_score, endpoint.c_applicability = 1.0, 0.75
    print(json.dumps({'scenario': 'c-real-gateway-to-retained-product-observations', 'state': 'green',
                      'routing_calls': 2, 'requested': 1, 'protection_opportunities': 1, 'suppressed': 1}), flush=True)
    return 2
