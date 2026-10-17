"""Opt-in C through real CLI, retained versions, publication and explicit diagnostics.

The only upstream is the caller's counted local deterministic HTTP fixture.
"""
import copy
import hashlib
import json
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
                      'state': 'green', 'explicit_calls': 3, 'implicit_calls': 0}), flush=True)
