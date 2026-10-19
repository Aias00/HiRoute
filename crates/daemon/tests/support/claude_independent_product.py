"""Independent settings through the real daemon/CLI; native clients and upstream are fixtures."""
import json
import os
from pathlib import Path
import stat
import subprocess
import sys

from model_connections_product import NativeUpstream, NATIVE_TOKEN, save_native_source
from publication_process import judgment_fixture, plan_change, ensure_native_model_evidence
from publication_product import Product


def apply(product, spec, key):
    command = 'agents restore' if spec['model']['intent'] == 'restore' else 'agents connect'
    ensure_native_model_evidence(product, 'agent_claude_default')
    preview = product.preview(command + ' preview', {'spec': spec})
    assert preview['applicable'], preview.get('blockers')
    body = {'spec': preview['spec'], 'accept_digest': preview['accept_digest'],
            'dependency_digest': preview['dependency_digest'], 'expected_revisions': preview['expected_revisions'],
            'idempotency_key': key}
    _, result = product.cli(command + ' apply', body)
    assert result['data']['state'] == 'succeeded', 'settings did not complete; inspect private frames'
    assert product.cli(command + ' apply', body)[1]['data'] == result['data'], 'replay changed the Operation'
    return preview, result['data']


def status(product):
    return product.preview('agents connect status', {'schema_version': {'major': 2, 'minor': 0},
                                                   'context_id': product.agent_context_id})


def snapshot(paths):
    return {str(path): (path.read_bytes(), stat.S_IMODE(path.stat().st_mode)) for path in paths}


def check(product, *, env=None):
    result = subprocess.run([str(product.bin / 'hiroute'), 'agent', 'launch', '--agent', 'claude-code', '--check'],
                            env=env or product.env, cwd=product.project, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=20)
    product.outputs.extend((result.stdout, result.stderr))
    return result.returncode, json.loads(result.stdout)['data']


def run(repository):
    product = Product(repository)
    upstream = NativeUpstream(base_host='localhost')
    try:
        product.install_codex_fixture()
        credentials = product.settings.with_name('.credentials.json')
        credentials.write_text('{"native_subscription":"fixture-stored-credential"}')
        credentials.chmod(0o600)
        linked = product.root / 'other-account-settings.json'
        linked.symlink_to(product.settings)
        product.start()
        saved = save_native_source(product, upstream, NATIVE_TOKEN, cli_input=True)
        product.public_cli('protected-input release --candidate candidate/native/product')
        product.editor = {'schema': 'hiroute.plan-editor/v2', 'display_name': 'Independent launch fixture',
                          'purpose': 'No native settings writes', 'mode': 'fixed_model',
                          'candidates': [{'binding_id': saved['binding_id']}],
                          'smart': {'economy': [], 'primary': [], 'judgment': judgment_fixture(),
                                    'reselect_on_user_message': False, 'classifier': {'kind': 'local_rules'}, 'complex_keywords': []},
                          'free': {'candidates': [], 'primary': [], 'primary_fallback': False},
                          'delegation_enabled': False, 'requirements': {},
                          'limits': {'maximum_attempts': 1, 'request_timeout_ms': 30000, 'attempt_timeout_ms': 30000}}
        change = plan_change(product, 'create', 'claude-independent-plan')
        preview = product.preview('routing preview', {'change': change})
        product.apply('routing apply', 'ApplyAgentPlanChange', preview, {'change': change}, 'claude-independent-plan')
        product.plan_id = preview['plan_head']['reference']['plan_id']
        scan = product.preview('agents scan')
        agent = next(agent for agent in scan['agents'] if agent['agent_id'] == 'agent_claude_default')
        product.agent_context_id = agent['context_id']
        product.agent_settings_agent_id = 'agent_claude_default'
        product.agent_connection = 'agent-connection/' + product.agent_context_id
        mapped = {'kind': 'plan', 'plan_id': product.plan_id}
        ordinary = {'schema_version': {'major': 2, 'minor': 0}, 'context_id': product.agent_context_id,
                    'model': {'intent': 'configure', 'settings': {'mode': 'claude_launcher', 'surfaces': ['claude_cli'],
                              'fixed_models': [], 'preset_mappings': {name: mapped for name in ('opus', 'sonnet', 'haiku')}}}}
        independent = dict(ordinary, activation_preference='managed_launch')
        apply(product, ordinary, 'ordinary-first')
        rejected = product.preview('agents connect preview', {'spec': independent})
        blockers = [block for block in rejected['blockers'] if block['reason'] == 'claude_connection_in_use']
        assert not rejected['applicable'] and blockers
        assert blockers[0]['connection_conflict']['connection_id'] == product.agent_connection
        assert blockers[0]['connection_conflict']['restore_point_ref'] == status(product)['restore_point_ref']
        restore = {'schema_version': {'major': 2, 'minor': 0}, 'context_id': product.agent_context_id,
                   'model': {'intent': 'restore', 'restore_point_ref': status(product)['restore_point_ref']}}
        apply(product, restore, 'ordinary-restore')
        native_before = snapshot([product.settings, credentials, linked])
        preview, accepted = apply(product, independent, 'independent-create')
        assert preview['activation_mode'] == 'managed_launch'
        assert snapshot([product.settings, credentials, linked]) == native_before
        operation = product.cli('operations get ' + accepted['operation_id'])[1]['data']
        # Native file effects are absent. The existing grant/publication transaction remains.
        assert 'agent-connection-managed-configuration' not in json.dumps(operation)
        current = status(product)
        assert current['state'] == 'configured' and current['activation_mode'] == 'managed_launch'
        rejected = product.preview('agents connect preview', {'spec': ordinary})
        assert not rejected['applicable'] and any(block['reason'] == 'claude_connection_in_use' for block in rejected['blockers'])
        code, ready = check(product)
        assert code == 0 and ready['launchable']
        user_bytes = product.settings.read_bytes()
        product.settings.write_bytes(b'not a JSON document')
        code, ready = check(product)
        assert code == 0 and ready['launchable'], 'launch attempted to reconstruct frozen state from user settings'
        product.settings.write_bytes(user_bytes)
        project = product.project / '.claude/settings.local.json'
        project.parent.mkdir(mode=0o700, exist_ok=True)
        project.write_text('{"env":{"ANTHROPIC_BASE_URL":"https://example.test/native"}}')
        project.chmod(0o600)
        code, blocked = check(product)
        assert code == 78 and blocked['checks'][-1]['reason_code'] == 'managed_launch_conflict'
        project.unlink()
        code, blocked = check(product, env=dict(product.env, CLAUDE_CODE_OAUTH_TOKEN='opaque-process-credential'))
        assert code == 78 and 'CLAUDE_CODE_OAUTH_TOKEN' in blocked['checks'][-1]['repair']
        assert 'opaque-process-credential' not in json.dumps(blocked)
        apply(product, independent, 'independent-update')
        assert snapshot([product.settings, credentials, linked]) == native_before
        product.stop()
        product.start()
        assert status(product)['state'] == 'configured'
        assert check(product)[0] == 0
        # Interrupt the existing publication transaction after the accepted state and grant
        # were staged. Recovery must consume that state, even if native user JSON is unreadable.
        product.stop()
        product.start('after_prepare')
        ensure_native_model_evidence(product, 'agent_claude_default')
        interrupted = dict(independent, access_token={'intent': 'regenerate'})
        frozen = product.preview('agents connect preview', {'spec': interrupted})
        assert frozen['applicable'], frozen.get('blockers')
        body = {'spec': frozen['spec'], 'accept_digest': frozen['accept_digest'],
                'dependency_digest': frozen['dependency_digest'], 'expected_revisions': frozen['expected_revisions'],
                'idempotency_key': 'independent-interrupted'}
        code, _ = product.cli('agents connect apply', body, success=False)
        assert code == 6 and product.process.wait(timeout=5) == 86
        product.stop(crash=True)
        user_bytes = product.settings.read_bytes()
        product.settings.write_bytes(b'unreadable native JSON during recovery')
        product.start()
        _, recovered = product.cli('agents connect apply', body)
        assert recovered['data']['state'] == 'succeeded'
        assert product.cli('agents connect apply', body)[1]['data'] == recovered['data']
        assert check(product)[0] == 0
        product.settings.write_bytes(user_bytes)
        assert snapshot([product.settings, credentials, linked]) == native_before
        restore['model']['restore_point_ref'] = status(product)['restore_point_ref']
        restored_preview, _ = apply(product, restore, 'independent-restore')
        assert restored_preview['spec']['activation_preference'] == 'managed_launch', 'old restore input must follow its sealed owner'
        assert status(product)['state'] == 'not_configured'
        assert snapshot([product.settings, credentials, linked]) == native_before
        assert check(product)[0] == 78
        print(json.dumps({'scenario': 'claude-independent-settings', 'state': 'green',
                          'native_files_unchanged': 3, 'cross_activation_refusals': 2,
                          'launch_blockers': 2, 'reopen': True, 'interrupted_recovery': True, 'live_model_calls': 0}), flush=True)
    finally:
        product.close()
        upstream.close()


if __name__ == '__main__':
    run(sys.argv[1])
